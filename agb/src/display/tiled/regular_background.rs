use core::{
    alloc::{Allocator, Layout},
    mem,
    ptr::NonNull,
};

use agb_fixnum::Vector2D;
use alloc::{vec, vec::Vec};

use crate::display::{tile_data::TileData, Priority};

use super::{
    BackgroundId, BackgroundIterator, RegularBackgroundData, ScreenblockAllocator, Tile,
    TileFormat, TileSet, TileSetting, SCREENBLOCK_SIZE, TRANSPARENT_TILE_INDEX, VRAM_MANAGER,
    VRAM_START,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum RegularBackgroundSize {
    Background32x32 = 0,
    Background64x32 = 1,
    Background32x64 = 2,
    Background64x64 = 3,
}

impl RegularBackgroundSize {
    const fn width(self) -> usize {
        match self {
            RegularBackgroundSize::Background32x32 => 32,
            RegularBackgroundSize::Background64x32 => 64,
            RegularBackgroundSize::Background32x64 => 32,
            RegularBackgroundSize::Background64x64 => 64,
        }
    }

    const fn height(self) -> usize {
        match self {
            RegularBackgroundSize::Background32x32 => 32,
            RegularBackgroundSize::Background64x32 => 32,
            RegularBackgroundSize::Background32x64 => 64,
            RegularBackgroundSize::Background64x64 => 64,
        }
    }

    const fn size_in_bytes(self) -> usize {
        self.num_tiles() * mem::size_of::<Tile>()
    }

    fn layout(self) -> Layout {
        Layout::from_size_align(self.size_in_bytes(), SCREENBLOCK_SIZE)
            .expect("failed to create layout, should never happen")
    }

    const fn num_tiles(self) -> usize {
        self.width() * self.height()
    }

    const fn gba_offset(self, pos: Vector2D<i32>) -> usize {
        let x_mod = (pos.x & (self.width() as i32 - 1)) as u32;
        let y_mod = (pos.y & (self.height() as i32 - 1)) as u32;

        let screenblock = (x_mod / 32) + (y_mod / 32) * (self.width() as u32 / 32);

        let pos = screenblock * 32 * 32 + (x_mod % 32 + 32 * (y_mod % 32));

        pos as usize
    }

    const fn size_flag(self) -> u16 {
        self as u16
    }
}

pub struct RegularBackgroundTiles {
    priority: Priority,
    size: RegularBackgroundSize,
    colours: TileFormat,

    tiles: Vec<Tile>,
    is_dirty: bool,

    scroll: Vector2D<i32>,

    screenblock_ptr: NonNull<Tile>,
}

impl RegularBackgroundTiles {
    #[must_use]
    pub fn new(priority: Priority, size: RegularBackgroundSize, colours: TileFormat) -> Self {
        let screenblock_ptr = ScreenblockAllocator
            .allocate(size.layout())
            .expect("Not enough space to allocate for background")
            .cast();

        Self {
            priority,
            size,
            colours,

            tiles: vec![Tile::default(); size.num_tiles()],
            is_dirty: true,

            scroll: Vector2D::default(),

            screenblock_ptr,
        }
    }

    pub fn set_scroll_pos(&mut self, scroll: impl Into<Vector2D<i32>>) {
        self.scroll = scroll.into();
    }

    #[must_use]
    pub fn scroll_pos(&self) -> Vector2D<i32> {
        self.scroll
    }

    pub fn set_tile(
        &mut self,
        pos: impl Into<Vector2D<i32>>,
        tileset: &TileSet<'_>,
        tile_setting: TileSetting,
    ) {
        assert_eq!(
            tileset.format(),
            self.colours,
            "Cannot set a {:?} colour tile on a {:?} colour background",
            tileset.format(),
            self.colours
        );

        let pos = self.size.gba_offset(pos.into());
        self.set_tile_at_pos(pos, tileset, tile_setting);
    }

    pub fn fill_with(&mut self, tile_data: &TileData) {
        assert!(
            tile_data.tile_settings.len() >= 20 * 30,
            "Don't have a full screen's worth of tile data"
        );

        assert_eq!(
            tile_data.tiles.format(),
            self.colours,
            "Cannot set a {:?} colour tile on a {:?} colour background",
            tile_data.tiles.format(),
            self.colours
        );

        for y in 0..20 {
            for x in 0..30 {
                let tile_id = y * 30 + x;
                let tile_pos = y * 32 + x;
                self.set_tile_at_pos(tile_pos, &tile_data.tiles, tile_data.tile_settings[tile_id]);
            }
        }
    }

    fn set_tile_at_pos(&mut self, pos: usize, tileset: &TileSet<'_>, tile_setting: TileSetting) {
        let old_tile = self.tiles[pos];
        if old_tile != Tile::default() {
            VRAM_MANAGER.remove_tile(old_tile.tile_index(self.colours));
        }

        let tile_index = tile_setting.index();

        let new_tile = if tile_index != TRANSPARENT_TILE_INDEX {
            let new_tile_idx = VRAM_MANAGER.add_tile(tileset, tile_index);
            Tile::new(new_tile_idx, tile_setting)
        } else {
            Tile::default()
        };

        if old_tile == new_tile {
            // no need to mark as dirty if nothing changes
            return;
        }

        self.tiles[pos] = new_tile;
        self.is_dirty = true;
    }

    pub fn commit(&mut self) {
        if self.is_dirty {
            unsafe {
                self.screenblock_ptr
                    .as_ptr()
                    .copy_from_nonoverlapping(self.tiles.as_ptr(), self.size.num_tiles());
            }
        }

        self.is_dirty = false;
    }

    pub fn show(&self, bg_iter: &mut BackgroundIterator<'_>) -> BackgroundId {
        bg_iter.set_next_regular(RegularBackgroundData {
            bg_ctrl: self.bg_ctrl_value(),
            scroll_offset: Vector2D::new(self.scroll.x as u16, self.scroll.y as u16),
        })
    }

    pub fn clear(&mut self) {
        for tile in &mut self.tiles {
            if *tile != Tile::default() {
                VRAM_MANAGER.remove_tile(tile.tile_index(self.colours));
            }

            *tile = Tile::default();
        }

        self.is_dirty = true;
    }

    #[must_use]
    pub fn size(&self) -> RegularBackgroundSize {
        self.size
    }

    fn bg_ctrl_value(&self) -> u16 {
        let tile_colour_flag: u16 = match self.colours {
            TileFormat::FourBpp => 0,
            TileFormat::EightBpp => 1,
        };

        self.priority as u16
            | (tile_colour_flag << 7)
            | (self.screen_base_block() << 8)
            | (self.size.size_flag() << 0xe)
    }

    fn screen_base_block(&self) -> u16 {
        let screenblock_location = self.screenblock_ptr.as_ptr() as usize;
        ((screenblock_location - VRAM_START) / SCREENBLOCK_SIZE) as u16
    }
}

impl Drop for RegularBackgroundTiles {
    fn drop(&mut self) {
        self.clear();
        unsafe { ScreenblockAllocator.deallocate(self.screenblock_ptr.cast(), self.size.layout()) };
    }
}
