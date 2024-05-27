#![no_std]
#![no_main]

use agb::{
    display::{
        object::{OamManaged, Object, Size, Sprite},
        palette16::Palette16,
        tiled::{RegularBackgroundSize, RegularBackgroundTiles, TileFormat, TileSet, TileSetting},
        HEIGHT, WIDTH,
    },
    input::Button,
};

#[derive(PartialEq, Eq)]
enum State {
    Ground,
    Upwards,
    Flapping,
}

struct Character<'a> {
    object: Object<'a>,
    position: Vector2D,
    velocity: Vector2D,
}

struct Vector2D {
    x: i32,
    y: i32,
}

fn tile_is_collidable(tile: u16) -> bool {
    let masked = tile & 0b0000001111111111;
    masked == 0 || masked == 4
}

fn frame_ranger(count: u32, start: u32, end: u32, delay: u32) -> usize {
    (((count / delay) % (end + 1 - start)) + start) as usize
}

#[agb::entry]
fn main(mut gba: agb::Gba) -> ! {
    let map_as_grid: &[[u16; 32]; 32] = unsafe {
        (&MAP_MAP as *const [u16; 1024] as *const [[u16; 32]; 32])
            .as_ref()
            .unwrap()
    };

    let (mut gfx, mut vram) = gba.display.video.tiled();
    let vblank = agb::interrupt::VBlank::get();
    let mut input = agb::input::ButtonController::new();

    vram.set_background_palette_raw(&MAP_PALETTE);
    let tileset = TileSet::new(&MAP_TILES, TileFormat::FourBpp);

    let mut background = RegularBackgroundTiles::new(
        agb::display::Priority::P0,
        RegularBackgroundSize::Background32x32,
        TileFormat::FourBpp,
    );

    for (i, &tile) in MAP_MAP.iter().enumerate() {
        let i = i as u16;
        background.set_tile(
            &mut vram,
            (i % 32, i / 32),
            &tileset,
            TileSetting::from_raw(tile & ((1 << 10) - 1), tile & !((1 << 10) - 1)),
        );
    }

    background.commit();

    let object = gba.display.object.get_managed();

    let sprite = object.sprite(&CHICKEN_SPRITES[0]);
    let mut chicken = Character {
        object: object.object(sprite),
        position: Vector2D {
            x: (6 * 8) << 8,
            y: ((7 * 8) - 4) << 8,
        },
        velocity: Vector2D { x: 0, y: 0 },
    };

    chicken
        .object
        .set_x((chicken.position.x >> 8).try_into().unwrap());
    chicken
        .object
        .set_y((chicken.position.y >> 8).try_into().unwrap());
    chicken.object.show();

    object.commit();

    let acceleration = 1 << 4;
    let gravity = 1 << 4;
    let flapping_gravity = gravity / 3;
    let jump_velocity = 1 << 9;
    let mut frame_count = 0;
    let mut frames_off_ground = 0;

    let terminal_velocity = (1 << 8) / 2;

    loop {
        frame_count += 1;

        input.update();

        // Horizontal movement
        chicken.velocity.x += (input.x_tri() as i32) * acceleration;
        chicken.velocity.x = 61 * chicken.velocity.x / 64;

        // Update position based on collision detection
        let state = handle_collision(
            &mut chicken,
            map_as_grid,
            gravity,
            flapping_gravity,
            terminal_velocity,
        );

        if state != State::Ground {
            frames_off_ground += 1;
        } else {
            frames_off_ground = 0;
        }

        // Jumping code
        if frames_off_ground < 10 && input.is_just_pressed(Button::A) {
            frames_off_ground = 200;
            chicken.velocity.y = -jump_velocity;
        }

        restrict_to_screen(&mut chicken);
        update_chicken_object(&mut chicken, &object, state, frame_count);

        let mut bg_iter = gfx.iter();
        background.show(&mut bg_iter);

        vblank.wait_for_vblank();
        bg_iter.commit(&mut vram);
        object.commit();
    }
}

fn update_chicken_object(
    chicken: &'_ mut Character<'_>,
    gfx: &OamManaged,
    state: State,
    frame_count: u32,
) {
    if chicken.velocity.x > 1 {
        chicken.object.set_hflip(false);
    } else if chicken.velocity.x < -1 {
        chicken.object.set_hflip(true);
    }
    match state {
        State::Ground => {
            if chicken.velocity.x.abs() > 1 << 4 {
                chicken
                    .object
                    .set_sprite(gfx.sprite(&CHICKEN_SPRITES[frame_ranger(frame_count, 1, 3, 10)]));
            } else {
                chicken.object.set_sprite(gfx.sprite(&CHICKEN_SPRITES[0]));
            }
        }
        State::Upwards => {}
        State::Flapping => {
            chicken
                .object
                .set_sprite(gfx.sprite(&CHICKEN_SPRITES[frame_ranger(frame_count, 4, 5, 5)]));
        }
    }

    let x: u16 = (chicken.position.x >> 8).try_into().unwrap();
    let y: u16 = (chicken.position.y >> 8).try_into().unwrap();

    chicken.object.set_x(x - 4);
    chicken.object.set_y(y - 4);
}

fn restrict_to_screen(chicken: &mut Character) {
    if chicken.position.x > (WIDTH - 8 + 4) << 8 {
        chicken.velocity.x = 0;
        chicken.position.x = (WIDTH - 8 + 4) << 8;
    } else if chicken.position.x < 4 << 8 {
        chicken.velocity.x = 0;
        chicken.position.x = 4 << 8;
    }
    if chicken.position.y > (HEIGHT - 8 + 4) << 8 {
        chicken.velocity.y = 0;
        chicken.position.y = (HEIGHT - 8 + 4) << 8;
    } else if chicken.position.y < 4 << 8 {
        chicken.velocity.y = 0;
        chicken.position.y = 4 << 8;
    }
}

fn handle_collision(
    chicken: &mut Character,
    map_as_grid: &[[u16; 32]; 32],
    gravity: i32,
    flapping_gravity: i32,
    terminal_velocity: i32,
) -> State {
    let mut new_chicken_x = chicken.position.x + chicken.velocity.x;
    let mut new_chicken_y = chicken.position.y + chicken.velocity.y;

    let tile_x = ((new_chicken_x >> 8) / 8) as usize;
    let tile_y = ((new_chicken_y >> 8) / 8) as usize;

    let left = (((new_chicken_x >> 8) - 4) / 8) as usize;
    let right = (((new_chicken_x >> 8) + 4) / 8) as usize;
    let top = (((new_chicken_y >> 8) - 4) / 8) as usize;
    let bottom = (((new_chicken_y >> 8) + 4) / 8) as usize;

    if chicken.velocity.x < 0 && tile_is_collidable(map_as_grid[tile_y][left]) {
        new_chicken_x = (((left + 1) * 8 + 4) << 8) as i32;
        chicken.velocity.x = 0;
    } else if chicken.velocity.x > 0 && tile_is_collidable(map_as_grid[tile_y][right]) {
        new_chicken_x = ((right * 8 - 4) << 8) as i32;
        chicken.velocity.x = 0;
    }

    if chicken.velocity.y < 0 && tile_is_collidable(map_as_grid[top][tile_x]) {
        new_chicken_y = ((((top + 1) * 8 + 4) << 8) + 4) as i32;
        chicken.velocity.y = 0;
    } else if chicken.velocity.y > 0 && tile_is_collidable(map_as_grid[bottom][tile_x]) {
        new_chicken_y = ((bottom * 8 - 4) << 8) as i32;
        chicken.velocity.y = 0;
    }

    let mut air_animation = State::Ground;

    if !tile_is_collidable(map_as_grid[bottom][tile_x]) {
        if chicken.velocity.y < 0 {
            air_animation = State::Upwards;
            chicken.velocity.y += gravity;
        } else {
            air_animation = State::Flapping;
            chicken.velocity.y += flapping_gravity;
            if chicken.velocity.y > terminal_velocity {
                chicken.velocity.y = terminal_velocity;
            }
        }
    }

    chicken.position.x = new_chicken_x;
    chicken.position.y = new_chicken_y;

    air_animation
}

// Below is the data for the sprites

static CHICKEN_PALETTE: Palette16 =
    Palette16::new([0x7C1E, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);

static CHICKEN_SPRITES: &[Sprite] = unsafe {
    &[
        Sprite::new(
            &CHICKEN_PALETTE,
            &[
                0x00, 0x00, 0x10, 0x01, 0x00, 0x00, 0x10, 0x11, 0x10, 0x00, 0x10, 0x01, 0x10, 0x11,
                0x11, 0x01, 0x10, 0x11, 0x11, 0x01, 0x00, 0x10, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00,
                0x00, 0x10, 0x01, 0x00,
            ],
            Size::S8x8,
        ),
        Sprite::new(
            &CHICKEN_PALETTE,
            &[
                0x00, 0x00, 0x10, 0x01, 0x00, 0x00, 0x10, 0x11, 0x10, 0x00, 0x10, 0x01, 0x10, 0x11,
                0x11, 0x01, 0x10, 0x11, 0x11, 0x01, 0x00, 0x01, 0x01, 0x00, 0x00, 0x01, 0x10, 0x00,
                0x10, 0x00, 0x00, 0x00,
            ],
            Size::S8x8,
        ),
        Sprite::new(
            &CHICKEN_PALETTE,
            &[
                0x00, 0x00, 0x10, 0x01, 0x00, 0x00, 0x10, 0x11, 0x10, 0x00, 0x10, 0x01, 0x10, 0x11,
                0x11, 0x01, 0x10, 0x11, 0x11, 0x01, 0x00, 0x10, 0x01, 0x00, 0x10, 0x01, 0x10, 0x00,
                0x00, 0x00, 0x10, 0x00,
            ],
            Size::S8x8,
        ),
        Sprite::new(
            &CHICKEN_PALETTE,
            &[
                0x00, 0x00, 0x10, 0x01, 0x00, 0x00, 0x10, 0x11, 0x10, 0x00, 0x10, 0x01, 0x10, 0x11,
                0x11, 0x01, 0x10, 0x11, 0x11, 0x01, 0x00, 0x10, 0x01, 0x00, 0x00, 0x11, 0x01, 0x00,
                0x00, 0x10, 0x00, 0x00,
            ],
            Size::S8x8,
        ),
        Sprite::new(
            &CHICKEN_PALETTE,
            &[
                0x00, 0x00, 0x10, 0x01, 0x00, 0x11, 0x11, 0x11, 0x10, 0x10, 0x11, 0x01, 0x10, 0x11,
                0x11, 0x01, 0x10, 0x11, 0x11, 0x01, 0x00, 0x10, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00,
                0x00, 0x00, 0x00, 0x00,
            ],
            Size::S8x8,
        ),
        Sprite::new(
            &CHICKEN_PALETTE,
            &[
                0x00, 0x00, 0x10, 0x01, 0x00, 0x00, 0x10, 0x11, 0x10, 0x11, 0x11, 0x01, 0x10, 0x11,
                0x11, 0x01, 0x10, 0x11, 0x11, 0x01, 0x00, 0x10, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00,
                0x00, 0x00, 0x00, 0x00,
            ],
            Size::S8x8,
        ),
    ]
};

static MAP_TILES: [u8; 8 * 17 * 4] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x01, 0x11, 0x11, 0x11, 0x01, 0x11, 0x11, 0x11, 0x01,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
    0x11, 0x11, 0x11, 0x11, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x11, 0x11, 0x00, 0x00, 0x10, 0x11,
    0x00, 0x01, 0x00, 0x11, 0x00, 0x11, 0x00, 0x10, 0x00, 0x11, 0x01, 0x00, 0x00, 0x11, 0x11, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x11, 0x01, 0x11, 0x00, 0x10, 0x01,
    0x11, 0x01, 0x00, 0x01, 0x11, 0x11, 0x00, 0x00, 0x11, 0x11, 0x01, 0x00, 0x11, 0x11, 0x11, 0x00,
    0x11, 0x11, 0x11, 0x00, 0x11, 0x11, 0x11, 0x00, 0x11, 0x11, 0x11, 0x00, 0x11, 0x11, 0x11, 0x00,
    0x11, 0x11, 0x11, 0x00, 0x11, 0x11, 0x11, 0x00, 0x11, 0x11, 0x00, 0x00, 0x11, 0x01, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x10, 0x11, 0x10, 0x10, 0x01,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x10, 0x10, 0x01,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x10, 0x11, 0x11, 0x01, 0x11, 0x10,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x10, 0x11,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
    0x11, 0x11, 0x11, 0x11, 0x01, 0x11, 0x11, 0x11, 0x11, 0x10, 0x11, 0x11, 0x11, 0x10, 0x11, 0x10,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x10, 0x11, 0x10,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x10, 0x11, 0x11, 0x10, 0x10, 0x11,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x01, 0x11, 0x11, 0x01, 0x01, 0x11,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x01, 0x11, 0x01, 0x01, 0x11,
];

static MAP_MAP: [u16; 1024] = [
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0002, 0x0003, 0x0003, 0x0003, 0x0003,
    0x0003, 0x0003, 0x0003, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0004,
    0x0404, 0x0004, 0x0404, 0x0004, 0x0404, 0x0004, 0x0404, 0x0004, 0x0404, 0x0004, 0x0404, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0804, 0x0C04, 0x0804, 0x0C04, 0x0804,
    0x0C04, 0x0804, 0x0C04, 0x0804, 0x0C04, 0x0804, 0x0C04, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0005, 0x0405, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0005, 0x0405, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0006,
    0x0406, 0x0002, 0x0003, 0x0003, 0x0003, 0x0003, 0x0003, 0x0003, 0x0001, 0x0006, 0x0406, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0000, 0x0000, 0x0004, 0x0404, 0x0004,
    0x0404, 0x0004, 0x0404, 0x0004, 0x0404, 0x0000, 0x0000, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0000, 0x0000, 0x0007, 0x0007, 0x0007, 0x0007, 0x0007, 0x0007, 0x0007,
    0x0007, 0x0000, 0x0000, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001,
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0008, 0x0009, 0x000A, 0x0000,
    0x0000, 0x000B, 0x000C, 0x000D, 0x000B, 0x000E, 0x0008, 0x0009, 0x000A, 0x0000, 0x0000, 0x000B,
    0x000B, 0x000C, 0x000D, 0x000B, 0x000E, 0x0008, 0x0009, 0x000A, 0x000F, 0x0010, 0x000B, 0x000C,
    0x000D, 0x000B, 0x000E, 0x0008, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000,
];

static MAP_PALETTE: [u16; 2] = [0x0000, 0x6A2F];
