use super::sfx::SfxPlayer;
use agb::display::{
    tiled::{
        RegularBackgroundSize, RegularBackgroundTiles, TileFormat, TiledBackground, VRAM_MANAGER,
    },
    Priority,
};

agb::include_background_gfx!(splash_screens,
    splash => deduplicate "gfx/splash.png",
    thanks_for_playing => deduplicate "gfx/thanks_for_playing.png",
);

pub enum SplashScreen {
    Start,
    End,
}

pub fn show_splash_screen(gfx: &mut TiledBackground<'_>, which: SplashScreen, sfx: &mut SfxPlayer) {
    let mut map = RegularBackgroundTiles::new(
        Priority::P3,
        RegularBackgroundSize::Background32x32,
        TileFormat::FourBpp,
    );

    map.set_scroll_pos((0i16, 0i16));
    let tile_data = match which {
        SplashScreen::Start => &splash_screens::splash,
        SplashScreen::End => &splash_screens::thanks_for_playing,
    };

    let vblank = agb::interrupt::VBlank::get();

    let mut input = agb::input::ButtonController::new();

    sfx.frame();
    vblank.wait_for_vblank();

    map.fill_with(tile_data);

    map.commit();
    VRAM_MANAGER.set_background_palettes(splash_screens::PALETTES);

    loop {
        let mut bg_iter = gfx.iter();
        map.show(&mut bg_iter);

        input.update();
        if input.is_just_pressed(
            agb::input::Button::A
                | agb::input::Button::B
                | agb::input::Button::START
                | agb::input::Button::SELECT,
        ) {
            break;
        }

        sfx.frame();
        vblank.wait_for_vblank();
        bg_iter.commit();
    }

    map.clear();
}
