//! A simplified implementation of the classic game "Breakout".
//!
//! Demonstrates Bevy's stepping capabilities if compiled with the `bevy_debug_stepping` feature.

use bevy::{
    math::bounding::{Aabb2d, BoundingCircle, BoundingVolume, IntersectsVolume}
};
use bevy::prelude::*;

mod stepping;
 
// These constants are defined in `Transform` units.
// Using the default 2D camera they correspond 1:1 with screen pixels.
const PADDLE_SIZE: Vec2 = Vec2::new(120.0, 20.0);
const GAP_BETWEEN_PADDLE_AND_FLOOR: f32 = 60.0;
const PADDLE_SPEED: f32 = 500.0;
// How close can the paddle get to the wall
const PADDLE_PADDING: f32 = 10.0;

// We set the z-value of the ball to 1 so it renders on top in the case of overlapping sprites.
const BALL_STARTING_POSITION: Vec3 = Vec3::new(0.0, -50.0, 1.0);
const BALL_DIAMETER: f32 = 30.;
const BALL_SPEED: f32 = 400.0;
const INITIAL_BALL_DIRECTION: Vec2 = Vec2::new(0.5, -0.5);

const WALL_THICKNESS: f32 = 10.0;
// x coordinates
const LEFT_WALL: f32 = -450.;
const RIGHT_WALL: f32 = 450.;
// y coordinates
const BOTTOM_WALL: f32 = -300.;
const TOP_WALL: f32 = 300.;

const BRICK_SIZE: Vec2 = Vec2::new(100., 30.);
// These values are exact
const GAP_BETWEEN_PADDLE_AND_BRICKS: f32 = 270.0;
const GAP_BETWEEN_BRICKS: f32 = 5.0;
// These values are lower bounds, as the number of bricks is computed
const GAP_BETWEEN_BRICKS_AND_CEILING: f32 = 20.0;
const GAP_BETWEEN_BRICKS_AND_SIDES: f32 = 20.0;

const SCOREBOARD_FONT_SIZE: f32 = 33.0;
const SCOREBOARD_TEXT_PADDING: Val = Val::Px(5.0);

const BACKGROUND_COLOR: Color = Color::srgb(0.9, 0.9, 0.9);
const PADDLE_COLOR: Color = Color::srgb(0.3, 0.3, 0.7);
const BALL_COLOR: Color = Color::srgb(1.0, 0.5, 0.5);
const BRICK_COLOR: Color = Color::srgb(0.5, 0.5, 1.0);
const WALL_COLOR: Color = Color::srgb(0.8, 0.8, 0.8);
const TEXT_COLOR: Color = Color::srgb(0.5, 0.5, 1.0);
const SCORE_COLOR: Color = Color::srgb(1.0, 0.5, 0.5);

const PROMPT_FONT_SIZE: f32 = 500.0;


fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(
            stepping::SteppingPlugin::default()
                .add_schedule(Update)
                .add_schedule(FixedUpdate)
                .at(Val::Percent(35.0), Val::Percent(50.0)),
        )
        .insert_resource(Score(0))
        .insert_resource(ClearColor(BACKGROUND_COLOR))
        .insert_resource(WarningTextTimer(Timer::from_seconds(1.0, TimerMode::Repeating)))
        .insert_resource(WarningTextIndex(0))
        .insert_resource(HideWindowTimer(Timer::from_seconds(3.0, TimerMode::Once)))
        .insert_resource(ShowImage(false)) // 新增资源用于标记是否显示图片
        .add_event::<CollisionEvent>()
        .add_systems(Startup, setup)
        // Add our gameplay simulation systems to the fixed timestep schedule
        // which runs at 64 Hz by default
        .add_systems(
            FixedUpdate,
            (
                apply_velocity,
                move_paddle,
                check_for_collisions,
                play_collision_sound,
            )
                .chain()
                .run_if(resource_exists_and_equals(GameRunning(true))),
        )
        .add_systems(Update, update_scoreboard)
        .add_systems(Update, 
            show_warning_text
                    .run_if(resource_exists_and_equals(GameRunning(false)))
                    .run_if(resource_exists_and_equals(GameEnd(false)))
                )
        .add_systems(Update, 
            handle_hide_window.run_if(resource_exists_and_equals(GameEnd(true)))
        )
        .add_systems(Update, 
            add_and_overlay_images.run_if(resource_exists_and_equals(GameEnd(true)))
        )
        .run();
}

#[derive(Component)]
struct Paddle;

#[derive(Component)]
struct Ball;

#[derive(Component, Deref, DerefMut)]
struct Velocity(Vec2);

#[derive(Event, Default)]
struct CollisionEvent;

#[derive(Component)]
struct Brick;

#[derive(Resource, Deref)]
struct CollisionSound(Handle<AudioSource>);

// Default must be implemented to define this as a required component for the Wall component below
#[derive(Component, Default)]
struct Collider;

// This is a collection of the components that define a "Wall" in our game
#[derive(Component)]
#[require(Sprite, Transform, Collider)]
struct Wall;

/// Which side of the arena is this wall located on?
enum WallLocation {
    Left,
    Right,
    Bottom,
    Top,
}

impl WallLocation {
    /// Location of the *center* of the wall, used in `transform.translation()`
    fn position(&self) -> Vec2 {
        match self {
            WallLocation::Left => Vec2::new(LEFT_WALL, 0.),
            WallLocation::Right => Vec2::new(RIGHT_WALL, 0.),
            WallLocation::Bottom => Vec2::new(0., BOTTOM_WALL),
            WallLocation::Top => Vec2::new(0., TOP_WALL),
        }
    }

    /// (x, y) dimensions of the wall, used in `transform.scale()`
    fn size(&self) -> Vec2 {
        let arena_height = TOP_WALL - BOTTOM_WALL;
        let arena_width = RIGHT_WALL - LEFT_WALL;
        // Make sure we haven't messed up our constants
        assert!(arena_height > 0.0);
        assert!(arena_width > 0.0);

        match self {
            WallLocation::Left | WallLocation::Right => {
                Vec2::new(WALL_THICKNESS, arena_height + WALL_THICKNESS)
            }
            WallLocation::Bottom | WallLocation::Top => {
                Vec2::new(arena_width + WALL_THICKNESS, WALL_THICKNESS)
            }
        }
    }
}

impl Wall {
    // This "builder method" allows us to reuse logic across our wall entities,
    // making our code easier to read and less prone to bugs when we change the logic
    // Notice the use of Sprite and Transform alongside Wall, overwriting the default values defined for the required components
    fn new(location: WallLocation) -> (Wall, Sprite, Transform) {
        (
            Wall,
            Sprite::from_color(WALL_COLOR, Vec2::ONE),
            Transform {
                // We need to convert our Vec2 into a Vec3, by giving it a z-coordinate
                // This is used to determine the order of our sprites
                translation: location.position().extend(0.0),
                // The z-scale of 2D objects must always be 1.0,
                // or their ordering will be affected in surprising ways.
                // See https://github.com/bevyengine/bevy/issues/4149
                scale: location.size().extend(1.0),
                ..default()
            },
        )
    }
}

// This resource tracks the game's score
#[derive(Resource, Deref, DerefMut)]
struct Score(usize);

#[derive(Component)]
struct ScoreboardUi;

// Add the game's entities to our world
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    _asset_server: Res<AssetServer>,
) {
    // Camera
    commands.spawn(Camera2d);

    // Sound
    let ball_collision_sound = _asset_server.load("sounds/breakout_collision.ogg");
    commands.insert_resource(CollisionSound(ball_collision_sound));
    commands.insert_resource(GameRunning(true));
    commands.insert_resource(GameEnd(false));
    // Paddle
    let paddle_y = BOTTOM_WALL + GAP_BETWEEN_PADDLE_AND_FLOOR;

    commands.spawn((
        Sprite::from_color(PADDLE_COLOR, Vec2::ONE),
        Transform {
            translation: Vec3::new(0.0, paddle_y, 0.0),
            scale: PADDLE_SIZE.extend(1.0),
            ..default()
        },
        Paddle,
        Collider,
    ));

    // Ball
    commands.spawn((
        Mesh2d(meshes.add(Circle::default())),
        MeshMaterial2d(materials.add(BALL_COLOR)),
        Transform::from_translation(BALL_STARTING_POSITION)
            .with_scale(Vec2::splat(BALL_DIAMETER).extend(1.)),
        Ball,
        Velocity(INITIAL_BALL_DIRECTION.normalize() * BALL_SPEED),
    ));

    // Scoreboard
    commands.spawn((
        Text::new("Score: "),
        TextFont {
            font_size: SCOREBOARD_FONT_SIZE,
            ..default()
        },
        TextColor(TEXT_COLOR),
        ScoreboardUi,
        Node {
            position_type: PositionType::Absolute,
            top: SCOREBOARD_TEXT_PADDING,
            left: SCOREBOARD_TEXT_PADDING,
            ..default()
        },
        children![(
            TextSpan::default(),
            TextFont {
                font_size: SCOREBOARD_FONT_SIZE,
                ..default()
            },
            TextColor(SCORE_COLOR),
        )],
    ));

    // Walls
    commands.spawn(Wall::new(WallLocation::Left));
    commands.spawn(Wall::new(WallLocation::Right));
    commands.spawn(Wall::new(WallLocation::Bottom));
    commands.spawn(Wall::new(WallLocation::Top));

    // Bricks
    let total_width_of_bricks = (RIGHT_WALL - LEFT_WALL) - 2. * GAP_BETWEEN_BRICKS_AND_SIDES;
    let bottom_edge_of_bricks = paddle_y + GAP_BETWEEN_PADDLE_AND_BRICKS;
    let total_height_of_bricks = TOP_WALL - bottom_edge_of_bricks - GAP_BETWEEN_BRICKS_AND_CEILING;

    assert!(total_width_of_bricks > 0.0);
    assert!(total_height_of_bricks > 0.0);

    // Given the space available, compute how many rows and columns of bricks we can fit
    let n_columns = (total_width_of_bricks / (BRICK_SIZE.x + GAP_BETWEEN_BRICKS)).floor() as usize;
    let n_rows = (total_height_of_bricks / (BRICK_SIZE.y + GAP_BETWEEN_BRICKS)).floor() as usize;
    let n_vertical_gaps = n_columns - 1;

    // Because we need to round the number of columns,
    // the space on the top and sides of the bricks only captures a lower bound, not an exact value
    let center_of_bricks = (LEFT_WALL + RIGHT_WALL) / 2.0;
    let left_edge_of_bricks = center_of_bricks
        // Space taken up by the bricks
        - (n_columns as f32 / 2.0 * BRICK_SIZE.x)
        // Space taken up by the gaps
        - n_vertical_gaps as f32 / 2.0 * GAP_BETWEEN_BRICKS;

    // In Bevy, the `translation` of an entity describes the center point,
    // not its bottom-left corner
    let offset_x = left_edge_of_bricks + BRICK_SIZE.x / 2.;
    let offset_y = bottom_edge_of_bricks + BRICK_SIZE.y / 2.;

    for row in 0..n_rows {
        for column in 0..n_columns {
            let brick_position = Vec2::new(
                offset_x + column as f32 * (BRICK_SIZE.x + GAP_BETWEEN_BRICKS),
                offset_y + row as f32 * (BRICK_SIZE.y + GAP_BETWEEN_BRICKS),
            );

            // brick
            commands.spawn((
                Sprite {
                    color: BRICK_COLOR,
                    ..default()
                },
                Transform {
                    translation: brick_position.extend(0.0),
                    scale: Vec3::new(BRICK_SIZE.x, BRICK_SIZE.y, 1.0),
                    ..default()
                },
                Brick,
                Collider,
            ));
        }
    }
    // // 使用相对路径加载图片资源
    // let texture_handle = asset_server.load("path.jpg");
    // // 插入 TextureToSpawn 资源
    // commands.insert_resource(TextureToSpawn(texture_handle)); 

}

fn move_paddle(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut paddle_transform: Single<&mut Transform, With<Paddle>>,
    time: Res<Time>,
) {
    let mut direction = 0.0;

    if keyboard_input.pressed(KeyCode::ArrowLeft) {
        direction -= 1.0;
    }

    if keyboard_input.pressed(KeyCode::ArrowRight) {
        direction += 1.0;
    }

    // Calculate the new horizontal paddle position based on player input
    let new_paddle_position =
        paddle_transform.translation.x + direction * PADDLE_SPEED * time.delta_secs();

    // Update the paddle position,
    // making sure it doesn't cause the paddle to leave the arena
    let left_bound = LEFT_WALL + WALL_THICKNESS / 2.0 + PADDLE_SIZE.x / 2.0 + PADDLE_PADDING;
    let right_bound = RIGHT_WALL - WALL_THICKNESS / 2.0 - PADDLE_SIZE.x / 2.0 - PADDLE_PADDING;

    paddle_transform.translation.x = new_paddle_position.clamp(left_bound, right_bound);
}

fn apply_velocity(mut query: Query<(&mut Transform, &Velocity)>, time: Res<Time>) {
    for (mut transform, velocity) in &mut query {
        transform.translation.x += velocity.x * time.delta_secs();
        transform.translation.y += velocity.y * time.delta_secs();
    }
}

fn update_scoreboard(
    score: Res<Score>,
    score_root: Single<Entity, (With<ScoreboardUi>, With<Text>)>,
    mut writer: TextUiWriter,
) {
    *writer.text(*score_root, 1) = score.to_string();
}

fn check_for_collisions(
    mut commands: Commands,
    mut score: ResMut<Score>,
    ball_query: Single<(&mut Velocity, &Transform), With<Ball>>,
    collider_query: Query<(Entity, &Transform, Option<&Brick>), With<Collider>>,
    mut collision_events: EventWriter<CollisionEvent>,
    mut game_running: ResMut<GameRunning>, // 添加 ResMut<GameRunning>
    
) {
    let (mut ball_velocity, ball_transform) = ball_query.into_inner();

    for (collider_entity, collider_transform, maybe_brick) in &collider_query {
        let collision = ball_collision(
            BoundingCircle::new(ball_transform.translation.truncate(), BALL_DIAMETER / 2.),
            Aabb2d::new(
                collider_transform.translation.truncate(),
                collider_transform.scale.truncate() / 2.,
            ),
        );

        if let Some(collision) = collision {
            // Writes a collision event so that other systems can react to the collision
            collision_events.write_default();

            // Bricks should be despawned and increment the scoreboard on collision
            if maybe_brick.is_some() {
                commands.entity(collider_entity).despawn();
                **score += 1;
            }

            // Reflect the ball's velocity when it collides
            let mut reflect_x = false;
            let mut reflect_y = false;

            // Reflect only if the velocity is in the opposite direction of the collision
            // This prevents the ball from getting stuck inside the bar
            match collision {
                Collision::Left => reflect_x = ball_velocity.x > 0.0,
                Collision::Right => reflect_x = ball_velocity.x < 0.0,
                Collision::Top => reflect_y = ball_velocity.y < 0.0,
                Collision::Bottom => reflect_y = ball_velocity.y > 0.0,
            }

            // Reflect velocity on the x-axis if we hit something on the x-axis
            if reflect_x {
                ball_velocity.x = -ball_velocity.x;
            }

            // Reflect velocity on the y-axis if we hit something on the y-axis
            if reflect_y {
                ball_velocity.y = -ball_velocity.y;
            }
        }
    }
     // Check if there are any bricks left
     let brick_count = collider_query.iter().filter(|(_, _, maybe_brick)| maybe_brick.is_some()).count();
     if brick_count == 0 {
         *game_running = GameRunning(false);
     }

     // 修改游戏运行状态
     if **score >= 2 { // 示例条件，当分数达到 10 时停止游戏
        game_running.0 = false;
    }
}

fn play_collision_sound(
    mut commands: Commands,
    mut collision_events: EventReader<CollisionEvent>,
    sound: Res<CollisionSound>,
) {
    // Play a sound once per frame if a collision occurred.
    if !collision_events.is_empty() {
        // This prevents events staying active on the next frame.
        collision_events.clear();
        commands.spawn((AudioPlayer(sound.clone()), PlaybackSettings::DESPAWN));
    }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
enum Collision {
    Left,
    Right,
    Top,
    Bottom,
}

// Returns `Some` if `ball` collides with `bounding_box`.
// The returned `Collision` is the side of `bounding_box` that `ball` hit.
fn ball_collision(ball: BoundingCircle, bounding_box: Aabb2d) -> Option<Collision> {
    if !ball.intersects(&bounding_box) {
        return None;
    }

    let closest = bounding_box.closest_point(ball.center());
    let offset = ball.center() - closest;
    let side = if offset.x.abs() > offset.y.abs() {
        if offset.x < 0. {
            Collision::Left
        } else {
            Collision::Right
        }
    } else if offset.y > 0. {
        Collision::Top
    } else {
        Collision::Bottom
    };

    Some(side)
}

#[derive(Resource, PartialEq)]
struct GameRunning(bool);

#[derive(Resource, PartialEq)]
struct GameEnd(bool);


#[derive(Component)]
struct PromptText;

// 新增警告文本计时器资源
#[derive(Resource)]
struct WarningTextTimer(Timer);

// 新增警告文本索引资源
#[derive(Resource)]
struct WarningTextIndex(usize);

// 显示警告文本的系统
fn show_warning_text(
    mut commands: Commands,
    mut timer: ResMut<WarningTextTimer>,
    mut index: ResMut<WarningTextIndex>,
    mut game_emd: ResMut<GameEnd>, 
    time: Res<Time>,
    mut text_query: Query<(Entity, &mut Text), With<PromptText>>,
) {
    timer.0.tick(time.delta());

    if timer.0.finished() {
        let warning_texts = ["RUN", "RUN", "RUN", "RUN", "!"];
        let current_text = warning_texts[index.0];

        // 移除之前的提示文本
        for (entity, _) in &mut text_query {
            commands.entity(entity).despawn();
        }

        if index.0 >=4 {
            game_emd.0 = true;
            return;
        }
        // 创建新的提示文本
        commands.spawn((
            Text(current_text.to_string()),
            bevy::ui::Node {
                position_type: bevy::ui::PositionType::Absolute,
                top: bevy::ui::Val::Percent(10.0),
                left: bevy::ui::Val::Percent(15.0),
                ..Default::default()
            },
            bevy::text::TextFont {
                font_size: PROMPT_FONT_SIZE,
                ..Default::default()
            },
            bevy::text::TextColor(Color::srgb(1.0, 0.0, 0.0)),
            PromptText,
        ));

        index.0 = (index.0 + 1) % warning_texts.len();
    }
}

// 新增隐藏窗口计时器资源
#[derive(Resource)]
struct HideWindowTimer(Timer);


// 新增资源用于标记是否显示图片
#[derive(Resource)]
struct ShowImage(bool);

// 处理隐藏窗口和显示图片的系统
fn handle_hide_window(
    _commands: Commands,
    mut hide_timer: ResMut<HideWindowTimer>,
    time: Res<Time>,
    _asset_server: Res<AssetServer>,
    mut windows: Query<&mut Window>, // 使用 Query<&mut Window>
) {
    // 启动隐藏窗口的倒计时
    if !hide_timer.0.finished() {
        hide_timer.0.tick(time.delta());
        // 隐藏窗口（移到屏幕外或缩小）
        for mut window in &mut windows {
            // 修改：直接设置 resolution 属性
            window.resolution.set(0.1, 0.1); // 缩小窗口到极小尺寸
        }
    } else {
            // 显示窗口并设置分辨率
        for mut window in &mut windows {
            // 修改：直接设置 resolution 属性
            window.resolution.set(1200.0, 800.0); // 缩小窗口到极小尺寸
        }
    }
}

// 新增资源用于存储待生成精灵的纹理句柄
#[derive(Resource)]
struct TextureToSpawn(Image);

//添加图片12312313232
fn add_and_overlay_images(mut commands: Commands, 
    asset_server: Res<AssetServer>,) {
    // commands.spawn(SpriteBundle {
    //     texture: asset_server.load("Breakout\\assets\path.jpeg"),
    //     transform: Transform::from_xyz(0.0, 0.0, 0.0),
    //     ..default()
    // });

    let icon = asset_server.load("not_open_1\\not_open\\path.jpg");
    commands.spawn((
        Node {
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        children![(
            ImageNode::new(icon),
            Node {
                // This will set the logo to be 200px wide and 200px high
                width: Val::Px(1000.0),
                height: Val::Px(1000.0),
                ..default()
            },
        )],
    ));

}