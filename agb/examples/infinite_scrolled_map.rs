#![no_std]
#![no_main]

use agb::{
    display::{
        tiled::{InfiniteScrolledMap, RegularBackgroundSize, RegularBackgroundTiles, VRAM_MANAGER},
        Priority,
    },
    include_background_gfx,
    input::ButtonController,
};
use agb_fixnum::vec2;

include_background_gfx!(big_map, "2ce8f4", big_map => deduplicate "examples/big_map.png");

#[agb::entry]
fn main(mut gba: agb::Gba) -> ! {
    let mut gfx = gba.display.video.tiled();
    let vblank = agb::interrupt::VBlank::get();

    let mut input = ButtonController::new();

    let tileset = &big_map::big_map.tiles;

    VRAM_MANAGER.set_background_palettes(big_map::PALETTES);

    let bg = RegularBackgroundTiles::new(
        Priority::P0,
        RegularBackgroundSize::Background32x32,
        tileset.format(),
    );

    let mut infinite_scrolled = InfiniteScrolledMap::new(bg);
    let mut current_pos = vec2(0, 0);

    loop {
        input.update();

        current_pos += input.vector();

        infinite_scrolled.set_pos(current_pos, |p| {
            (
                &big_map::big_map.tiles,
                big_map::big_map.tile_settings
                    [p.x.rem_euclid(60) as usize + p.y.rem_euclid(40) as usize * 60],
            )
        });

        let mut bg_iter = gfx.iter();
        infinite_scrolled.show(&mut bg_iter);

        vblank.wait_for_vblank();
        infinite_scrolled.commit();
        bg_iter.commit();
    }
}
