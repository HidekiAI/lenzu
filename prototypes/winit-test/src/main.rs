use image::GenericImageView;
use image::DynamicImage;
use winit::dpi::PhysicalSize;
use winit::raw_window_handle::HasDisplayHandle;
use winit::raw_window_handle::HasWindowHandle;
use winit::window::Window;
use winit::{
    event::{ElementState, Event, KeyEvent, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    keyboard::{self, Key, KeyCode, PhysicalKey},
    window::WindowBuilder,
};

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let window = WindowBuilder::new().build(&event_loop).unwrap();
    // start off with the window hidden in case garbage is displayed
    window.set_visible(false);

    // ControlFlow::Poll continuously runs the event loop, even if the OS hasn't
    // dispatched any events. This is ideal for games and similar applications.
    event_loop.set_control_flow(ControlFlow::Poll);

    // ControlFlow::Wait pauses the event loop if no events are available to process.
    // This is ideal for non-game applications that only update in response to user
    // input, and uses significantly less power/CPU time than ControlFlow::Poll.
    event_loop.set_control_flow(ControlFlow::Wait);

    // load PNG image
    let image = image::open("../../assets/ubunchu01_02_panel01_section_02.png").unwrap();

    // render it on the window
    let (width, height) = image.dimensions();
    // Create a new image buffer with the same dimensions as the window
    let mut buffer = image::ImageBuffer::new(width, height);

    // Copy the pixels from the loaded image to the buffer
    for (x, y, pixel) in buffer.enumerate_pixels_mut() {
        let image_pixel = image.get_pixel(x, y);
        *pixel = image::Rgba([image_pixel[0], image_pixel[1], image_pixel[2], image_pixel[3]]);
    }

    // Create a dynamic image from the buffer
    let dynamic_image = DynamicImage::ImageRgba8(buffer);

    // Get the window's inner size
    let size = window.inner_size();

    // Resize the dynamic image to fit the window
    let resized_image = dynamic_image.resize(size.width, size.height, image::imageops::FilterType::Lanczos3);

    // Convert the resized image to a buffer
    let resized_buffer = resized_image.as_bytes();

    // get window handle
    let window_handle = window.window_handle().expect("Cannot get window handle");

    // get display handle
    let display_handle = window.display_handle().expect("Cannot get display handle");




    // now show the window
    window.set_visible(true);

    let run_result = event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                println!("The close button was pressed; stopping");
                elwt.exit();
            }
            Event::AboutToWait => {
                // Application update code.

                // Queue a RedrawRequested event.
                //
                // You only need to call this if you've determined that you need to redraw in
                // applications which do not always need to. Applications that redraw continuously
                // can render here instead.
                window.request_redraw();
            }
            Event::WindowEvent {
                event: WindowEvent::RedrawRequested,
                ..
            } => {
                // Redraw the application.
                //
                // It's preferable for applications that do not render continuously to render in
                // this event rather than in AboutToWait, since rendering in here allows
                // the program to gracefully handle redraws requested by the OS.
            }
            Event::WindowEvent {
                event:
                    WindowEvent::KeyboardInput {
                        event: key_event, ..
                    },
                ..
            } => {
                println!("Key pressed: {:?}", key_event);
                match key_event {
                    // Escape key:
                    //  KeyEvent { physical_key: Code(Escape), logical_key: Named(Escape), text: None, location: Standard, state: Released, repeat: false, platform_specific: KeyEventExtra { text_with_all_modifers: None, key_without_modifiers: Named(Escape) } }
                    KeyEvent {
                        state: ElementState::Released,
                        physical_key: PhysicalKey::Code(KeyCode::Escape),
                        ..
                    } => {
                        println!("The escape key was pressed; stopping");
                        elwt.exit();
                    }
                    _ => (),
                }
            }
            _ => (),
        }
    });

    match run_result {
        Ok(_) => println!("Event loop exited cleanly"),
        Err(e) => eprintln!("Error occurred in event loop: {}", e),
    }
}
