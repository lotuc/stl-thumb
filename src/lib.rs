extern crate cgmath;
#[macro_use]
extern crate glium;
extern crate image;
extern crate mint;

pub mod config;
mod mesh;

use std::error::Error;
use std::fs::File;
use std::{thread, time};
use config::Config;
use cgmath::EuclideanSpace;
use glium::{glutin, Surface};
use mesh::Mesh;

// TODO: Move this stuff to config module
const BACKGROUND_COLOR: (f32, f32, f32, f32) = (1.0, 1.0, 1.0, 0.0);
const CAM_FOV_DEG: f32 = 30.0;
const CAM_POSITION: cgmath::Point3<f32> = cgmath::Point3 {x: 2.0, y: 4.0, z: 2.0};


fn view_matrix(position: cgmath::Point3<f32>, direction: cgmath::Vector3<f32>, up: cgmath::Vector3<f32>) -> [[f32; 4]; 4] {
    // TODO: Use matrix math in here
    let f = {
        let f = direction;
        let len = f[0] * f[0] + f[1] * f[1] + f[2] * f[2];
        let len = len.sqrt();
        [f[0] / len, f[1] / len, f[2] / len]
    };

    let s = [up[1] * f[2] - up[2] * f[1],
             up[2] * f[0] - up[0] * f[2],
             up[0] * f[1] - up[1] * f[0]];

    let s_norm = {
        let len = s[0] * s[0] + s[1] * s[1] + s[2] * s[2];
        let len = len.sqrt();
        [s[0] / len, s[1] / len, s[2] / len]
    };

    let u = [f[1] * s_norm[2] - f[2] * s_norm[1],
             f[2] * s_norm[0] - f[0] * s_norm[2],
             f[0] * s_norm[1] - f[1] * s_norm[0]];

    let p = [-position[0] * s_norm[0] - position[1] * s_norm[1] - position[2] * s_norm[2],
             -position[0] * u[0] - position[1] * u[1] - position[2] * u[2],
             -position[0] * f[0] - position[1] * f[1] - position[2] * f[2]];

    [
        [s_norm[0], u[0], f[0], 0.0],
        [s_norm[1], u[1], f[1], 0.0],
        [s_norm[2], u[2], f[2], 0.0],
        [p[0], p[1], p[2], 1.0],
    ]
}


pub fn run(config: &Config) -> Result<(), Box<Error>> {
    // Create geometry from STL file
    // =========================

    // TODO: Add support for URIs instead of plain file names
    // https://developer.gnome.org/integration-guide/stable/thumbnailer.html.en
    let stl_file = File::open(&config.stl_filename)?;
    let mesh = Mesh::from_stl(stl_file)?;


    // Graphics Stuff
    // ==============

    // Create GL context
    // -----------------

    let mut events_loop = glutin::EventsLoop::new();
    let window = glutin::WindowBuilder::new()
        .with_title("stl-thumb")
        .with_dimensions(config.width, config.height)
        .with_min_dimensions(config.width, config.height)
        .with_max_dimensions(config.width, config.height)
        .with_visibility(config.visible);
    let context = glutin::ContextBuilder::new()
        .with_depth_buffer(24);
        //.with_gl(glutin::GlRequest::Specific(glutin::Api::OpenGlEs, (2, 0)));
    let display = glium::Display::new(window, context, &events_loop).unwrap();
    //let context = glutin::HeadlessRendererBuilder::new(config.width, config.height)
    //    //.with_depth_buffer(24)
    //    .build().unwrap();
    //let display = glium::HeadlessRenderer::new(context).unwrap();

    // Print context information
    println!("GL Version:   {:?}", display.get_opengl_version());
    println!("GL Version:   {}", display.get_opengl_version_string());
    println!("GLSL Version: {:?}", display.get_supported_glsl_version());
    println!("Vendor:       {}", display.get_opengl_vendor_string());
    println!("Renderer      {}", display.get_opengl_renderer_string());
    println!("Free GPU Mem: {:?}", display.get_free_video_memory());


    let params = glium::DrawParameters {
        depth: glium::Depth {
            test: glium::draw_parameters::DepthTest::IfLess,
            write: true,
            .. Default::default()
        },
        backface_culling: glium::draw_parameters::BackfaceCullingMode::CullCounterClockwise,
        .. Default::default()
    };

    // Load and compile shaders
    // ------------------------

    let vertex_shader_src = include_str!("model.vert");
    let pixel_shader_src = include_str!("model.frag");

    // TODO: Cache program binary
    let program = glium::Program::from_source(&display, &vertex_shader_src, &pixel_shader_src, None);
    let program = match program {
        Ok(p) => p,
        Err(glium::CompilationError(err)) => {
            eprintln!("{}",err);
            panic!("Compiling shaders");
        },
        Err(err) => panic!("{}",err),
    };

    // Send mesh data to GPU
    // ---------------------

    let vertex_buf = glium::VertexBuffer::new(&display, &mesh.vertices).unwrap();
    let normal_buf = glium::VertexBuffer::new(&display, &mesh.normals).unwrap();
    // Can use NoIndices here because STLs are dumb
    let indices = glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList);

    // Setup uniforms
    // --------------

    // Transformation matrix (positions, scales and rotates model)
    let transform_matrix = mesh.scale_and_center();

    // View matrix (convert to positions relative to camera)
    let view = view_matrix(CAM_POSITION, cgmath::Point3::origin()-CAM_POSITION, cgmath::Vector3::unit_z());

    // Perspective matrix (give illusion of depth)
    let perspective = {
        let (width, height) = (config.width, config.height);
        let aspect_ratio = height as f32 / width as f32;

        let fov = CAM_FOV_DEG.to_radians();
        let zfar = 1024.0;
        let znear = 0.1;

        let f = 1.0 / (fov / 2.0).tan();

        [
            [f * aspect_ratio, 0.0,                            0.0, 0.0],
            [             0.0,   f,                            0.0, 0.0],
            [             0.0, 0.0,      (zfar+znear)/(zfar-znear), 1.0],
            [             0.0, 0.0, -(2.0*zfar*znear)/(zfar-znear), 0.0],
        ]
    };

    // Direction of light source
    let light = [-1.4, 0.4, -0.7f32];

    let uniforms = uniform! {
        model: Into::<[[f32; 4]; 4]>::into(transform_matrix),
        view: view,
        perspective: perspective,
        u_light: light,
    };

    // Draw
    // ----

    // Create off screen texture to render to
    let texture = glium::Texture2d::empty(&display, config.width, config.height).unwrap();
    let depthtexture = glium::texture::DepthTexture2d::empty(&display, config.width, config.height).unwrap();
    let mut framebuffer = glium::framebuffer::SimpleFrameBuffer::with_depth_buffer(&display, &texture, &depthtexture).unwrap();

    // Fills background color and clears depth buffer
    framebuffer.clear_color_and_depth(BACKGROUND_COLOR, 1.0);
    framebuffer.draw((&vertex_buf, &normal_buf), &indices, &program, &uniforms, &params)
        .unwrap();
    // TODO: Antialiasing
    // TODO: Shadows

    // Save Image
    // ==========

    let pixels: glium::texture::RawImage2d<u8> = texture.read();
    let img = image::ImageBuffer::from_raw(config.width, config.height, pixels.data.into_owned()).unwrap();
    let img = image::DynamicImage::ImageRgba8(img).flipv();
    let mut output = std::fs::File::create(&config.img_filename).unwrap();
    img.write_to(&mut output, image::ImageFormat::PNG)
        .expect("Error saving image");

    // Wait until window is closed
    // ===========================

    if config.visible {
        let mut closed = false;
        let sleep_time = time::Duration::from_millis(10);
        while !closed {
            thread::sleep(sleep_time);
            // Copy framebuffer to display
            let target = display.draw();
            target.blit_from_simple_framebuffer(&framebuffer,
                                                &glium::Rect {
                                                    left: 0,
                                                    bottom: 0,
                                                    width: config.width,
                                                    height: config.height,
                                                },
                                                &glium::BlitTarget {
                                                    left: 0,
                                                    bottom: 0,
                                                    width: config.width as i32,
                                                    height: config.height as i32,
                                                },
                                                glium::uniforms::MagnifySamplerFilter::Nearest);
            target.finish().unwrap();
            // Listing the events produced by the application and waiting to be received
            events_loop.poll_events(|ev| {
                match ev {
                    glutin::Event::WindowEvent { event, .. } => match event {
                        glutin::WindowEvent::Closed => closed = true,
                        _ => (),
                    },
                    _ => (),
                }
            });
        }
    }

    //let mut window = three::Window::new(env!("CARGO_PKG_NAME"));
    //window.scene.background = three::Background::Color(0xFFFFFF);

    //println!("== STL ==");
    //mesh.debug();
    //let material = three::material::Phong {
    //    color: 0x00A0ff,
    //    glossiness: 80.0,
    //};
    //let mesh = window.factory.mesh(geometry, material);
    //window.scene.add(&mesh);

    ////let camera = window.factory.orthographic_camera(cam_center, yextent, zrange);
    //let camera = window.factory.perspective_camera(CAM_FOV_DEG, 1.0..500.0);
    ////let cam_pos = [150.0, -150.0, 150.0];
    //let cam_pos = locate_camera(&bounds);
    //camera.set_position(cam_pos);
    //camera.look_at(cam_pos, center, None);

    // Plane
    //let plane = {
    //    let geometry = Geometry::plane(
    //        (bounds.max.x - bounds.min.x) * 3.0,
    //        (bounds.max.y - bounds.min.y) * 3.0,
    //    );
    //    let material = three::material::Lambert {
    //        //color: 0xA0ffA0,
    //        color: 0xffffff,
    //        flat: false,
    //    };
    //    window.factory.mesh(geometry, material)
    //};
    //plane.set_position([center[0], center[1], bounds.min.z]);
    //window.scene.add(&plane);

    // Test sphere
    //let sphere = {
    //    let geometry = Geometry::uv_sphere(40.0, 20, 20);
    //    println!("== Sphere ==");
    //    debug_geo(&geometry);
    //    let material = three::material::Phong {
    //        color: 0xffA0A0,
    //        glossiness: 80.0,
    //    };
    //    window.factory.mesh(geometry, material)
    //};
    //sphere.set_position([30.0, 40.0, 2.5]);
    //window.scene.add(&sphere);

    // Lights
    //let hemisphere_light = window.factory.hemisphere_light(0xffffff, 0x8080ff, 0.5);
    //window.scene.add(&hemisphere_light);
    // TODO: Change dir_light position and shadow map size based on size of object
    //let mut dir_light = window.factory.directional_light(0xffffff, 0.9);
    //dir_light.look_at([-100.0, -400.0, 350.0], [0.0, 0.0, 0.0], None);
    //let shadow_map = window.factory.shadow_map(2048, 2048);
    //dir_light.set_shadow(shadow_map, 400.0, 1.0..1000.0);
    //window.scene.add(&dir_light);
    //let ambient_light = window.factory.ambient_light(0xdc8874, 0.5);
    //window.scene.add(ambient_light);
    //let point_light = window.factory.point_light(0xffffff, 0.5);
    //point_light.set_position([150.0, 350.0, 350.0]);
    //window.scene.add(point_light);

    //while window.update() {
    //    window.render(&camera);
    //}

    Ok(())
}

// TODO: Move tests to their own file
#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::ErrorKind;
    use super::*;

    #[test]
    fn cube() {
        let config = Config {
            stl_filename: "test_data/cube.stl".to_string(),
            img_filename: "cube.png".to_string(),
            width: 1024,
            height: 768,
            visible: false,
        };

        match fs::remove_file(&config.img_filename) {
            Ok(_) => (),
            Err(ref error) if error.kind() == ErrorKind::NotFound => (),
            Err(_) => {
                panic!("Couldn't clean files before testing");
            }
        }

        run(&config).expect("Error in run function");

        let size = fs::metadata(config.img_filename)
            .expect("No file created")
            .len();

        assert_ne!(0, size);
    }
}
