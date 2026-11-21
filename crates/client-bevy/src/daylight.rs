use super::{Options, network::Online};
use bevy::{
    asset::RenderAssetUsages,
    core_pipeline::tonemapping::Tonemapping,
    light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap, Skybox},
    prelude::*,
    render::render_resource::{
        Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
    },
};
#[derive(Component)]
pub struct ForestSun;
const HORIZON: Vec3 = Vec3::new(0.74, 0.82, 0.80);
const SOLAR: Vec3 = Vec3::new(0.48, 0.65, -0.59);
fn sky(images: &mut Assets<Image>) -> Handle<Image> {
    const W: u32 = 96;
    let mut pixels = Vec::with_capacity((W * W * 6 * 4) as usize);
    for face in 0..6 {
        for j in 0..W {
            for i in 0..W {
                let u = 2. * (i as f32 + 0.5) / W as f32 - 1.;
                let v = 2. * (j as f32 + 0.5) / W as f32 - 1.;
                let d = match face {
                    0 => Vec3::new(1., -v, -u),
                    1 => Vec3::new(-1., -v, u),
                    2 => Vec3::new(u, 1., v),
                    3 => Vec3::new(u, -1., -v),
                    4 => Vec3::new(u, -v, 1.),
                    _ => Vec3::new(-u, -v, -1.),
                }
                .normalize();
                let h = d.y.max(0.).sqrt();
                let sun = (d * Vec3::new(1., 1., -1.)).dot(SOLAR.normalize()).max(0.);
                let horizon = HORIZON;
                let zenith = Vec3::new(0.33, 0.56, 0.74);
                let color = horizon
                    .lerp(zenith, h)
                    .lerp(Vec3::new(1., 0.87, 0.64), sun.powf(27.) * 0.6)
                    .lerp(
                        Vec3::new(1., 0.97, 0.79),
                        ((sun - 0.9988) * 1100.).clamp(0., 1.),
                    );
                pixels.extend(color.to_array().map(|c| (c * 255.) as u8));
                pixels.push(255);
            }
        }
    }
    let mut im = Image::new(
        Extent3d {
            width: W,
            height: W,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    im.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    images.add(im)
}
#[allow(clippy::too_many_arguments)]
pub fn sync(
    mut cmds: Commands,
    o: Res<Options>,
    online: Option<Res<Online>>,
    cam: Single<Entity, With<Camera3d>>,
    sun: Query<Entity, With<ForestSun>>,
    mut images: ResMut<Assets<Image>>,
    mut prior: Local<Option<bool>>,
) {
    let forest = if let Some(n) = online {
        n.0.lock().unwrap().is_forest()
    } else {
        o.preset == Some(primal_voxel::Preset::ForestModern)
    };
    if *prior == Some(forest) {
        return;
    }
    *prior = Some(forest);
    if !forest {
        cmds.entity(*cam)
            .remove::<(DistanceFog, Skybox, bevy::camera::Hdr)>()
            .insert(Tonemapping::None);
        for e in &sun {
            cmds.entity(e).despawn();
        }
        cmds.insert_resource(GlobalAmbientLight::default());
        return;
    }
    let profile = o.style;
    if profile.lighting {
        cmds.insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.68, 0.82, 1.),
            brightness: 380.,
            ..default()
        });
        cmds.insert_resource(DirectionalLightShadowMap {
            size: profile.shadow_size,
        });
        cmds.spawn((
            ForestSun,
            DirectionalLight {
                color: Color::srgb(1., 0.89, 0.70),
                illuminance: 12500.,
                shadow_maps_enabled: profile.shadows > 0,
                shadow_depth_bias: 0.018,
                shadow_normal_bias: 0.65,
                ..default()
            },
            Transform::default().looking_to(-SOLAR, Vec3::Y),
            CascadeShadowConfigBuilder {
                num_cascades: profile.shadows.max(1),
                maximum_distance: profile.shadow_distance,
                first_cascade_far_bound: profile.shadow_distance * 0.36,
                ..default()
            }
            .build(),
        ));
        cmds.entity(*cam).insert((
            Skybox {
                image: Some(sky(&mut images)),
                // The fog is applied in exposed scene-linear space. Match its horizon.
                brightness: 1. / bevy::camera::Exposure::default().exposure(),
                ..default()
            },
            profile.tone,
            bevy::camera::Hdr,
        ));
    }
    if profile.fog {
        cmds.entity(*cam).insert(DistanceFog {
            color: Color::srgb(HORIZON.x, HORIZON.y, HORIZON.z),
            directional_light_color: Color::srgba(0.98, 0.86, 0.66, 0.16),
            directional_light_exponent: 12.,
            falloff: FogFalloff::Linear {
                start: (o.radius * 16) as f32 * 0.39,
                end: (o.radius * 16) as f32 * 0.91,
            },
        });
    }
}
