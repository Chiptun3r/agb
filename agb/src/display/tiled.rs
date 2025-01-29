mod affine_background;
mod infinite_scrolled_map;
mod regular_background;
mod vram_manager;

use core::marker::PhantomData;

pub use super::affine::AffineMatrixBackground;
pub use affine_background::{
    AffineBackgroundSize, AffineBackgroundTiles, AffineBackgroundWrapBehaviour,
};
pub use infinite_scrolled_map::{InfiniteScrolledMap, PartialUpdateStatus};
pub use regular_background::{RegularBackgroundSize, RegularBackgroundTiles};
pub use vram_manager::{DynamicTile, TileFormat, TileIndex, TileSet, VRAM_MANAGER};

use crate::{
    agb_alloc::{block_allocator::BlockAllocator, bump_allocator::StartEnd, impl_zst_allocator},
    dma::DmaControllable,
    fixnum::{Num, Vector2D},
    memory_mapped::MemoryMapped,
};

use super::DISPLAY_CONTROL;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BackgroundId(pub(crate) u8);

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AffineBackgroundId(pub(crate) u8);

impl BackgroundId {
    #[must_use]
    pub fn x_scroll_dma(self) -> DmaControllable<u16> {
        unsafe { DmaControllable::new((0x0400_0010 + self.0 as usize * 4) as *mut _) }
    }
}

const TRANSPARENT_TILE_INDEX: u16 = 0xffff;

#[derive(Clone, Copy, Debug, Default)]
#[repr(align(4))]
pub struct TileSetting {
    tile_id: u16,
    effect_bits: u16,
}

impl TileSetting {
    pub const BLANK: Self = TileSetting::new(TRANSPARENT_TILE_INDEX, false, false, 0);

    #[must_use]
    pub const fn new(tile_id: u16, hflip: bool, vflip: bool, palette_id: u8) -> Self {
        Self {
            tile_id,
            effect_bits: ((hflip as u16) << 10)
                | ((vflip as u16) << 11)
                | ((palette_id as u16) << 12),
        }
    }

    #[must_use]
    pub const fn from_raw(tile_id: u16, effect_bits: u16) -> Self {
        Self {
            tile_id,
            effect_bits,
        }
    }

    #[must_use]
    pub const fn hflip(self, should_flip: bool) -> Self {
        Self {
            effect_bits: self.effect_bits ^ ((should_flip as u16) << 10),
            ..self
        }
    }

    #[must_use]
    pub const fn vflip(self, should_flip: bool) -> Self {
        Self {
            effect_bits: self.effect_bits ^ ((should_flip as u16) << 11),
            ..self
        }
    }

    #[must_use]
    pub const fn palette(self, palette_id: u8) -> Self {
        Self {
            effect_bits: self.effect_bits ^ ((palette_id as u16) << 12),
            ..self
        }
    }

    fn index(self) -> u16 {
        self.tile_id
    }

    fn setting(self) -> u16 {
        self.effect_bits
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
struct Tile(u16);

impl Tile {
    fn new(idx: TileIndex, setting: TileSetting) -> Self {
        Self(idx.raw_index() | setting.setting())
    }

    fn tile_index(self, format: TileFormat) -> TileIndex {
        TileIndex::new(self.0 as usize & ((1 << 10) - 1), format)
    }
}

struct ScreenblockAllocator;

pub(crate) const VRAM_START: usize = 0x0600_0000;
pub(crate) const SCREENBLOCK_SIZE: usize = 0x800;
pub(crate) const CHARBLOCK_SIZE: usize = SCREENBLOCK_SIZE * 8;

const SCREENBLOCK_ALLOC_START: usize = VRAM_START + CHARBLOCK_SIZE * 2;

static SCREENBLOCK_ALLOCATOR: BlockAllocator = unsafe {
    BlockAllocator::new(StartEnd {
        start: || SCREENBLOCK_ALLOC_START,
        end: || SCREENBLOCK_ALLOC_START + 0x4000,
    })
};

impl_zst_allocator!(ScreenblockAllocator, SCREENBLOCK_ALLOCATOR);

#[derive(Default)]
struct RegularBackgroundData {
    bg_ctrl: u16,
    scroll_offset: Vector2D<u16>,
}

#[derive(Default)]
struct AffineBackgroundData {
    bg_ctrl: u16,
    scroll_offset: Vector2D<Num<i32, 8>>,
    affine_transform: AffineMatrixBackground,
}

pub struct TiledBackground<'gba> {
    _phantom: PhantomData<&'gba ()>,
}

impl TiledBackground<'_> {
    pub(crate) unsafe fn new() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }

    pub fn iter(&mut self) -> BackgroundIterator<'_> {
        BackgroundIterator::default()
    }
}

#[derive(Default)]
pub struct BackgroundIterator<'bg> {
    _phantom: PhantomData<&'bg ()>,

    num_regular: usize,
    regular_backgrounds: [RegularBackgroundData; 4],

    num_affine: usize,
    affine_backgrounds: [AffineBackgroundData; 2],
}

impl BackgroundIterator<'_> {
    fn set_next_regular(&mut self, data: RegularBackgroundData) -> BackgroundId {
        let bg_index = self.next_regular_index();

        self.regular_backgrounds[bg_index] = data;
        BackgroundId(bg_index as u8)
    }

    fn next_regular_index(&mut self) -> usize {
        if self.num_regular + self.num_affine * 2 >= 4 {
            panic!(
                "Can only have 4 backgrounds at once, affine counts as 2. regular: {}, affine: {}",
                self.num_regular, self.num_affine
            );
        }

        let index = self.num_regular;
        self.num_regular += 1;
        index
    }

    fn set_next_affine(&mut self, data: AffineBackgroundData) -> AffineBackgroundId {
        let bg_index = self.next_affine_index();

        self.affine_backgrounds[bg_index - 2] = data;
        AffineBackgroundId(bg_index as u8)
    }

    fn next_affine_index(&mut self) -> usize {
        if self.num_affine * 2 + self.num_regular >= 3 {
            panic!(
                "Can only have 4 backgrounds at once, affine counts as 2. regular: {}, affine: {}",
                self.num_regular, self.num_affine
            );
        }

        let index = self.num_affine;
        self.num_affine += 1;

        index + 2 // first affine BG is bg2
    }

    pub fn commit(self) {
        let video_mode = self.num_affine as u16;
        let enabled_backgrounds =
            ((1u16 << self.num_regular) - 1) | (((1 << self.num_affine) - 1) << 2);

        let mut display_control = DISPLAY_CONTROL.get();

        display_control &= 0b1111000001111000;
        display_control |= video_mode | (enabled_backgrounds << 8);

        DISPLAY_CONTROL.set(display_control);

        for (i, regular_background) in self
            .regular_backgrounds
            .iter()
            .take(self.num_regular)
            .enumerate()
        {
            let bg_ctrl = unsafe { MemoryMapped::new(0x0400_0008 + i * 2) };
            bg_ctrl.set(regular_background.bg_ctrl);

            let bg_x_offset = unsafe { MemoryMapped::new(0x0400_0010 + i * 4) };
            bg_x_offset.set(regular_background.scroll_offset.x);
            let bg_y_offset = unsafe { MemoryMapped::new(0x0400_0012 + i * 4) };
            bg_y_offset.set(regular_background.scroll_offset.y);
        }

        for (i, affine_background) in self
            .affine_backgrounds
            .iter()
            .take(self.num_affine)
            .enumerate()
        {
            let i = i + 2;

            let bg_ctrl = unsafe { MemoryMapped::new(0x0400_0008 + i * 2) };
            bg_ctrl.set(affine_background.bg_ctrl);

            let bg_x_offset = unsafe { MemoryMapped::new(0x0400_0028 + (i - 2) * 16) };
            bg_x_offset.set(affine_background.scroll_offset.x.to_raw());
            let bg_y_offset = unsafe { MemoryMapped::new(0x0400_002c + (i - 2) * 16) };
            bg_y_offset.set(affine_background.scroll_offset.y.to_raw());

            let affine_transform_offset = unsafe { MemoryMapped::new(0x0400_0020 + (i - 2) * 16) };
            affine_transform_offset.set(affine_background.affine_transform);
        }

        VRAM_MANAGER.gc();
    }
}
