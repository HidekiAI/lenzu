mod capture;
mod interpreter;
mod ocr;

use crate::{glib::clone, interpreter::interpreter_traits::InterpreterTraitResult};
use capture::{
    capture_traits::{CaptureRect, CaptureTrait, HandleTypes},
    capture_winapi::{self, CaptureWinApi},
    capture_x11::CaptureX11,
};

use gdk::Key;
use gdk4_win32::{
    ffi::{gdk_win32_surface_get_impl_hwnd, GdkWin32Surface},
    Win32Surface, HWND,
};
use glib::translate::ToGlibPtr;
// NOTE: make sure to 'cargo add' glib for graphene_point_t
use gtk4::{
    ffi::{
        gtk_list_store_append, gtk_widget_compute_transform, GtkButton, GtkEventController,
        GtkEventControllerKeyClass, GtkWidget,
    },
    gdk::{self, Backend, Display},
    gdk_pixbuf::Pixbuf,
    gio::{
        self,
        ffi::{g_application_bind_busy_property, g_application_quit},
    },
    glib,
    graphene::{self, ffi::graphene_point_t},
    prelude::{WidgetExt, *},
    subclass::widget,
    ApplicationWindow, Button, HeaderBar, Image, Orientation, Picture, Widget,
};

// NOTE: Are we using imageproc::ImageBuffer or image::ImageBuffer?
use image::{imageops::overlay, DynamicImage, Rgba, *};
use imageproc::image::{self, GenericImageView, ImageBuffer};

use interpreter::{interpreter_ja_mecab::InterpreterJaMecab, interpreter_traits::InterpreterTrait};
use ocr::{
    image_handling::OCRImage, ocr_tesseract::OcrTesseract, ocr_traits::OcrTrait,
    ocr_winmedia::OcrWinMedia,
};

use once_cell::sync::Lazy;
use std::{
    boxed::Box,
    cell::RefCell,
    ffi::CString,
    ptr,
    rc::Rc,
    sync::{Arc, OnceLock, RwLock},
};
use tokio::runtime::Runtime;

const GTK_APP_ID: &str = "com.github.hidekiai.lenzu";
const GTK_APP_PATH: &str = "/com/github/hidekiai/lenzu/";
const DEFAULT_WINDOW_WIDTH: i32 = 1024;
const DEFAULT_WINDOW_HEIGHT: i32 = 768;
const DEFAULT_LENS_WINDOW_SIZE: i32 = 512; // squre (both width/height)

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ToggleState {
    Free,
    MoveWindow,
    Capture,
}

//static mut TOGGLE_STATE: Rc<RefCell<ToggleState>> = Rc::new(RefCell::new(ToggleState::Free));
//static TOGGLE_STATE: Lazy<Arc<RefCell<ToggleState>>> = Lazy::new(|| Arc::new(RefCell::new(ToggleState::Free))); // Error: Sync is not implemented
static TOGGLE_STATE: Lazy<Arc<RwLock<ToggleState>>> =
    Lazy::new(|| Arc::new(RwLock::new(ToggleState::Free)));
fn update_toggle_state() {
    let binding = TOGGLE_STATE.clone();
    let mut toggle_state = binding.write().unwrap();
    match *toggle_state {
        ToggleState::Free => {
            *toggle_state = ToggleState::MoveWindow;
            println!("ToggleState: MoveWindow");
        }
        ToggleState::MoveWindow => {
            *toggle_state = ToggleState::Capture;
            println!("ToggleState: Capture");
        }
        ToggleState::Capture => {
            *toggle_state = ToggleState::Free;
            println!("ToggleState: Captured");
        }
    }
}

fn create_ocr(args: &Vec<String>) -> std::boxed::Box<dyn OcrTrait> {
    let force_windows_ocr = cfg!(target_os = "windows");
    if force_windows_ocr {
        // even if tesseract is installed, if on Windows, use the most reliable OCR available instead if no arguments are passed
        if args.len() > 1 && args[1] != "--use-winmedia-ocr" {
            return std::boxed::Box::new(OcrTesseract::new()); //  if the first arg is not --use-winmedia-ocr, then use Tesseract
        }
        // just use default windows OCR
        return std::boxed::Box::new(OcrWinMedia::new());
    }
    // Not on Windows, so use Tesseract OCR
    std::boxed::Box::new(OcrTesseract::new()) // default to Tesseract (because even if it unreliable, at least it is cross-platform and can be used on Linux)
}

fn create_interpreter(_args: &Vec<String>) -> std::boxed::Box<dyn InterpreterTrait> {
    std::boxed::Box::new(InterpreterJaMecab::new())
}

fn create_capture(args: &Vec<String>) -> std::boxed::Box<dyn CaptureTrait> {
    if cfg!(target_os = "windows") {
        std::boxed::Box::new(CaptureWinApi::new())
    } else {
        std::boxed::Box::new(CaptureX11::new())
    }
}

fn capture_and_ocr(
    ocr: &mut std::boxed::Box<dyn OcrTrait>,
    capture: &mut std::boxed::Box<dyn CaptureTrait>,
    ocr_font: &mut OCRImage,
    _supported_lang: &str, // '+' separated list of supported languages(i.e. "jpn+jpn_ver+osd"), note that longer this list, longer it takes to OCR (ie. 10sec/lang so if there are 4 in this list, it can take 40 seconds!)
    interpreter: &mut std::boxed::Box<dyn InterpreterTrait>,
) {
    // now capture the screen
    let rect: Option<CaptureRect> = None; // TODO: pass in the rect
    let screenshot = capture.capture(rect).unwrap();

    // the image we just captured, we'll need to now pass it down to OCR and get the text back
    // We will (for now) assume it is either "jpn" or "jpn_vert" and we'll just pass it down
    // to kakasi and convert all kanji to hiragana
    // 1. convert to grayscale
    // 2. pass it down to OCR
    // 3. get the text back
    // 4. draw the text onto the mem_dc_topmost
    // 5. blend the topmost layer onto the primary image
    // 6. scale/magnify
    // 7. draw the magnified image onto the window
    // convert DC to RGBA - probably can get away with 24-bit but for better byte alignment, will stay at 32-bit
    let gray_scale_image = screenshot.grayscale(); // Convert the image to grayscale
    let ocr_start_time = std::time::Instant::now();
    let ocr_result = ocr.evaluate(&gray_scale_image);
    let ocr_time = ocr_start_time.elapsed().as_millis();

    // now run kakasi to convert the kanji to hiragana
    // Translate Japanese text to hiragana
    let start_interpreter = std::time::Instant::now();
    let possible_result_tupled = match ocr_result {
        Ok(recognized_result) => {
            println!("OCR Result: '{:?}' {} mSec", recognized_result, ocr_time);

            for line in recognized_result.lines.clone() {
                if line.contains(" ") {
                    panic!("evaluate_async(): Detected space in line: {:?}", line);
                }
                // dump each char as bytes
                for c in line.chars() {
                    print!("{:?} ", c as u8);
                }
                println!("");
            }
            let possible_translate_result = interpreter.convert(&recognized_result.lines);
            match possible_translate_result {
                Ok(translate_result) => {
                    println!(
                        "Interpreter Result: '{:?}' {} mSec",
                        translate_result,
                        start_interpreter.elapsed().as_millis()
                    );
                    Some((recognized_result, translate_result))
                }
                Err(e) => {
                    println!(
                        "Error: {:?} - {} mSec",
                        e,
                        start_interpreter.elapsed().as_millis()
                    );
                    Some((recognized_result, InterpreterTraitResult::new()))
                }
            }
        }
        Err(e) => {
            println!("Error: {:?} - {} mSec", e, ocr_time);
            None
        }
    };
    match possible_result_tupled {
        Some((recognized_result, translate_result)) => {
            println!(
                "########################## Interpreter Result ({} mSec):\n'{}'\n'{}'\n",
                start_interpreter.elapsed().as_millis(),
                recognized_result,
                translate_result,
            );

            // And then, layer this PNG onto the original image (blend  png_buffer onto gray_scale_image)
            // image width and height is based on max of the two
            // now create a PNG with alpha channel and draw the text onto the image
            let mut recognized_image = screenshot;
            if !translate_result.text.is_empty() {
                ocr_font.set_image(recognized_image);
                recognized_image = ocr_font.overlay_text(translate_result.text.as_str(), 0, 0);
            }

            if cfg!(debug_assertions) {
                // save the image for debugging purposes
                println!("Saving debug image: recognized_image.png");
                recognized_image.save("recognized_image.png").unwrap();
            }

            // render translated text onto the window
            capture.render(recognized_image);
        }
        None => {
            println!(
                "Interpreter Result ({} mSec): '{:?}'",
                start_interpreter.elapsed().as_millis(),
                possible_result_tupled
            );
            // render what we've captured originally instead
            capture.render(screenshot);
        }
    }
}

fn quit_signal_sender_thread() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| Runtime::new().expect("Setting up tokio runtime needs to succeed."))
}

fn main() -> glib::ExitCode {
    let application = gtk4::Application::builder()
        .application_id(GTK_APP_ID)
        .build();
    application.connect_activate(build_ui);
    application.run()
}

fn build_ui(application: &gtk4::Application) {
    // default to Tesseract OCR, but if  --use-winmedia-ocr is passed, then use Windows.Media.Ocr
    let args: &Vec<String> = &std::env::args().collect();
    let mut ocr = create_ocr(&args);
    let ocr_langugages = ocr.init();
    let mut interpreter = create_interpreter(&args);
    let mut ocr_font = OCRImage::new(None);
    let mut capture = create_capture(&args);

    let app_window_gtk: ApplicationWindow = gtk4::ApplicationWindow::builder()
        .application(application)
        .title("Lenzu")
        .default_width(DEFAULT_WINDOW_WIDTH)
        .default_height(DEFAULT_WINDOW_HEIGHT)
        .build();
    let window_scrollable = gtk4::ScrolledWindow::builder().build();
    window_scrollable.set_visible(true);
    window_scrollable.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Automatic);
    app_window_gtk.set_child(Some(&window_scrollable));

    // container to append multiple children
    let parent_box = gtk4::Box::new(Orientation::Vertical, 0);

    // as Picture
    let image_path = "recognized_image.png"; // if it exists, load last used image
    let picture = Picture::for_filename(image_path);
    let pic_paintable_dim = match picture.paintable() {
        Some(paintable) => (paintable.intrinsic_width(), paintable.intrinsic_height()),
        None => (1024, 768),
    };
    println!(
        "Loaded image '{}' with dimensions: {:?}",
        image_path, pic_paintable_dim
    );
    picture.set_halign(gtk4::Align::Center);
    picture.set_size_request(pic_paintable_dim.0, pic_paintable_dim.1);
    picture.set_visible(true);
    parent_box.append(&picture);

    // as Image
    let image = Image::from_file(image_path);
    let img_paintable_dim = match image.paintable() {
        Some(paintable) => (paintable.intrinsic_width(), paintable.intrinsic_height()),
        None => (0, 0),
    };
    println!(
        "Loaded image '{}' with dimensions: {:?}",
        image_path, img_paintable_dim
    );
    image.set_halign(gtk4::Align::Center);
    image.set_size_request(img_paintable_dim.0, img_paintable_dim.1);
    image.set_visible(true);
    //parent_box.append(&image);

    let (sender_quit_signal, receiver_quit_signal): (
        async_channel::Sender<bool>,
        async_channel::Receiver<bool>,
    ) = async_channel::bounded(1);

    // Create a button with label and margins
    let button_quit: Button = Button::builder()
        .label("Quit")
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .width_request(16 * 16)
        .height_request(16)
        .halign(gtk4::Align::End) // anchor to bottom right
        .valign(gtk4::Align::End)
        .build();
    button_quit.connect_clicked(move |_| {
        println!("Signal quitting...");
        quit_signal_sender_thread().spawn(clone!(@strong sender_quit_signal  =>async move {
            sender_quit_signal .send(true) .await.expect("Signal channel is unopenend");
        }));
        println!("Signal sent to quit...");
    });
    parent_box.append(&button_quit);
    glib::spawn_future_local(clone!(@weak button_quit => async move {
        while let Ok(quit_signaled) = receiver_quit_signal.recv().await {
            if quit_signaled {
                button_quit.set_label("Quitting...");

                // quit applications
                std::process::exit(0); // for now, brute-force quit, in future will elegantly signal for exit...
                //break;
            }
        }
    }));

    // create a sub-window for use as a lens - it's non-modal, floating, and barely visible
    // lens does not need to have a renderer since the transparancies acts like what is
    // under the window is what is being focused on
    let lens_window = gtk4::Window::builder()
        .application(application) // application stays alive as long as any of windows associated to it is alive
        //.parent(Some(&app_window_gtk)) //  hopefully, this will make it so it becomes a sibling to main window
        .destroy_with_parent(true)
        .default_width(DEFAULT_LENS_WINDOW_SIZE)
        .default_height(DEFAULT_LENS_WINDOW_SIZE)
        .opacity(0.25) // barely can see it and see-through so that it is not in the way
        .decorated(false) // no title bar
        .modal(false) // non-modal because this window has no decorations and if it is modal, it will block the main window on closing or moving them
        .visible(true)
        .build();

    // monitor for pointer move or hover events over lens_window; first connect "key_press_event" to Window
    // Note that add_controller() method only exists for gtk_widget_add_controller(GtkEventController)
    // and GtkShortcutManager.add_controller(Gtk.ShortcutController(base:GtkEventController))
    // to register for GtkControllerEvent.  So first, cast what we are interested in to Widget
    let app_win_widget: Widget = app_window_gtk.clone().upcast::<gtk4::Widget>();
    let app_win_gtkwdiget: *mut GtkWidget = app_win_widget.to_glib_none().0;
    let lens_as_widget: gtk4::Widget = lens_window.clone().upcast::<Widget>(); // Widget == WidgetExt
    let lens_gtkwidget: *mut GtkWidget = lens_as_widget.to_glib_none().0;
    // box trait WidgetExt (std::boxed::Box<dyn gtk4::prelude::WidgetExt>) for gtk4::Widgets?

    // guint keycode = gdk_key_event_get_keycode(event);
    // guint keyval = gdk_key_event_get_keyval(event);
    // gboolean isModifier = gdk_key_event_is_modifier(event);
    // gboolean matches = gdk_key_event_matches(event, GDK_KEY_C, GDK_CONTROL_MASK);
    // if (matches) {
    //     // Handle Ctrl-C shortcut
    // }
    // // We want to ignore irrelevant modifiers like ScrollLock
    // #define ALL_ACCELS_MASK (GDK_CONTROL_MASK | GDK_SHIFT_MASK | GDK_ALT_MASK)
    // state = gdk_event_get_modifier_state (event);
    // gdk_keymap_translate_keyboard_state (keymap,
    //                                      gdk_key_event_get_keycode (event),
    //                                      state,
    //                                      gdk_key_event_get_group (event),
    //                                      &keyval, NULL, NULL, &consumed);
    // if (keyval == GDK_PLUS &&
    //     (state & ~consumed & ALL_ACCELS_MASK) == GDK_CONTROL_MASK)
    //   // Control was pressed

    //register gtk_widget_add_controller(GtkEventController)
    let event_controller_key: gtk4::EventControllerKey =
        gtk4::EventControllerKey::builder().build();
    // before we transfer owneship, setup signal/event handler
    event_controller_key.connect_key_released(
        |event_controller_key, keyval, keycode_raw, state| {
            // space key hit: Key released: control=EventControllerKey { inner: TypedObjectRef { inner: 0x27a62d99440, type: GtkEventControllerKey } } keyval=Key(32) keycode=32 state=Modi
            println!(
                "Key released: control={:?} keyval={:?} keycode={:?} state={:?}",
                event_controller_key, keyval, keycode_raw, state
            );
            match keyval {
                Key::Escape => {
                    // quit
                    println!("> connect_key_released(Key::Escape) - Quitting...");
                    std::process::exit(0); // for now, brute-force quit, in future will elegantly signal for exit...
                }
                Key::space => {
                    println!("> connect_key_released(Key::Space) - ToggleState...");
                    // toggle between free, move window, and capture
                    update_toggle_state();
                }
                _ => {
                    println!("> connect_key_released() - Unhandled keyval: {:?}", keyval);
                }
            }
        },
    );
    app_win_widget.add_controller(gtk4::EventController::from(event_controller_key)); // transfer ownership to Widget once callback is in place...

    // Event for mouse pointer movement (we only care if it is in ToggleState::MoveWindow)
    let event_controller_motion: gtk4::EventControllerMotion =
        gtk4::EventControllerMotion::builder().build();
    event_controller_motion.connect_motion(|event_controller_motion, x: f64, y: f64| {
        println!(
            "Mouse moved: {:?} x={}, y={}",
            event_controller_motion, x, y
        );
        let toggle_state_rc = TOGGLE_STATE.clone();
        let toggle_state = toggle_state_rc.read().unwrap();
        if *toggle_state == ToggleState::MoveWindow {
            // set center of lens window to mouse cursor
            let x32 = x as f32;
            let y32 = y as f32;
            let current_point: graphene_point_t = graphene_point_t { x: x32, y: y32 };
        }
    });
    app_win_widget.add_controller(gtk4::EventController::from(event_controller_motion)); // transfer ownership to Widget once callback is in place...

    // let the lens window set it's coordinate/position based on the mouse cursor
    //lens_window.connect_move_focus(focus_lens_window);
    let gesture_single = gtk4::GestureSingle::from(gtk4::GestureClick::new()); // handling mouse events and single-touch gestures

    //GtkWidget *somewidget; // Your GtkWidget instance
    //gint wx, wy; gtk_widget_translate_coordinates(somewidget, gtk_widget_get_toplevel(somewidget), 0, 0, &wx, &wy); // Now wx and wy contain the absolute position of somewidget
    let current_point: graphene_point_t = graphene_point_t {
        x: DEFAULT_LENS_WINDOW_SIZE as f32 / 2.0,
        y: DEFAULT_LENS_WINDOW_SIZE as f32 / 2.0,
    };
    let mut out_point: graphene_point_t = graphene_point_t { x: 0.0, y: 0.0 };
    let _compute_success = unsafe {
        gtk4::ffi::gtk_widget_compute_point(
            app_win_gtkwdiget,
            lens_gtkwidget,
            &current_point,
            &mut out_point,
        )
    };
    println!("Lens window position: ({}, {})", out_point.x, out_point.y);

    // all is attached to parent_box, now attach itself to window
    //app_window_gtk.set_child(Some(&parent_box));
    window_scrollable.set_child(Some(&parent_box));
    app_window_gtk.set_visible(true);
    app_window_gtk.present(); // mark (child scene-graph nodes) for refresh

    // all is setup, now pass the window structure/model to Display
    capture.init(&app_window_gtk); // IMPORTANT:  init() attempts to extract gdk4_<desktop>::Surface::Handle() (i.e. HWND), it MUST be called AFTER the window has been presented() so that the GdkSurface exists!!!

    // Create an action for quitting
    let quit_action = gio::SimpleAction::new("quit", None);
    quit_action.connect_activate(move |_, _| {
        app_window_gtk.close();
    });
    application.add_action(&quit_action);
}

#[cfg(test)]
mod tests {
    use super::*;
    // NOTE: We want to use imageproc::image rather than image crate because we want to use imageproc::drawing::draw_text_mut()
    use imageproc::{
        drawing::draw_text_mut,
        image::{self, GrayAlphaImage},
    };

    #[test]
    fn test_text_over_image() {
        let mut ocr_image = OCRImage::new(None);
        ocr_image.load_image("../assets/ubunchu01_02.png").unwrap();
        let is_valid = ocr_image.is_png();
        println!("Is valid PNG: {}", is_valid);

        // and turn those bytes into a DynamicImage
        println!("Overlaying text onto image...");
        let result_bytes =
            ocr_image.overlay_text("最近人気の\nデスクトップな\nリナックスです!", 0, 0);
        // Now you can use `result_bytes` as needed (e.g., send it over the network, etc.)

        // save it as a file for visual confirmation
        result_bytes.save("test_text_over_image.png").unwrap();
    }

    #[test]
    fn test_draw_text_mut() {
        // Create a new blank image
        let ocr_image = OCRImage::from(GrayAlphaImage::new(1024, 768));
        //let mut img: ImageBuffer<image::LumaA<u8>, Vec<u8>> = GrayAlphaImage::new(1024, 768);
        //let img = ImageBuffer::from(ocr_image.get_image().to_luma8());
        let mut canvas = ocr_image.get_image().to_luma_alpha8();

        // Draw some text onto the image
        draw_text_mut(
            &mut canvas,
            image::LumaA([255, 0x7f]), // font color
            0,                         // font x position
            0,                         // font y position
            24.0,                      // font scale
            &ocr_image.get_font_bold(),
            "最近人気の\nデスクトップな\nリナックスです!", // text to draw
        );

        let img =
            image::GrayAlphaImage::from_raw(canvas.width(), canvas.height(), canvas.into_raw())
                .unwrap();
        img.save("test_draw_text_mut.png").unwrap();
    }
}

/*
// short-cuts takes keypress of X and Ctrl-G
#include <gtk/gtk.h>

static GtkWidget *window = NULL;

static gboolean
shortcut_activated (GtkWidget *widget,
                    GVariant  *unused,
                    gpointer   row)
{
  g_print ("activated %s\n", gtk_label_get_label (row));
  return TRUE;
}

static GtkShortcutTrigger *
create_ctrl_g (void)
{
  return gtk_keyval_trigger_new (GDK_KEY_g, GDK_CONTROL_MASK);
}

static GtkShortcutTrigger *
create_x (void)
{
  return gtk_keyval_trigger_new (GDK_KEY_x, 0);
}

struct {
  const char *description;
  GtkShortcutTrigger * (* create_trigger_func) (void);
} shortcuts[] = {
  { "Press Ctrl-G", create_ctrl_g },
  { "Press X", create_x },
};

GtkWidget *
do_shortcut_triggers (GtkWidget *do_widget)
{
  guint i;

  if (!window)
    {
      GtkWidget *list;
      GtkEventController *controller;

      window = gtk_window_new ();
      gtk_window_set_display (GTK_WINDOW (window),
                              gtk_widget_get_display (do_widget));
      gtk_window_set_title (GTK_WINDOW (window), "Shortcuts");
      gtk_window_set_default_size (GTK_WINDOW (window), 200, -1);
      gtk_window_set_resizable (GTK_WINDOW (window), FALSE);
      g_object_add_weak_pointer (G_OBJECT (window), (gpointer *)&window);

      list = gtk_list_box_new ();
      gtk_widget_set_margin_top (list, 6);
      gtk_widget_set_margin_bottom (list, 6);
      gtk_widget_set_margin_start (list, 6);
      gtk_widget_set_margin_end (list, 6);
      gtk_window_set_child (GTK_WINDOW (window), list);

      for (i = 0; i < G_N_ELEMENTS (shortcuts); i++)
        {
          GtkShortcut *shortcut;
          GtkWidget *row;

          row = gtk_label_new (shortcuts[i].description);
          gtk_list_box_insert (GTK_LIST_BOX (list), row, -1);

          controller = gtk_shortcut_controller_new ();
          gtk_shortcut_controller_set_scope (GTK_SHORTCUT_CONTROLLER (controller), GTK_SHORTCUT_SCOPE_GLOBAL);
          gtk_widget_add_controller (row, controller);

          shortcut = gtk_shortcut_new (shortcuts[i].create_trigger_func(),
                                       gtk_callback_action_new (shortcut_activated, row, NULL));
          gtk_shortcut_controller_add_shortcut (GTK_SHORTCUT_CONTROLLER (controller), shortcut);
        }
    }

  if (!gtk_widget_get_visible (window))
    gtk_widget_set_visible (window, TRUE);
  else
    gtk_window_destroy (GTK_WINDOW (window));

  return window;
}


*/

/*
// PAINT
#include <glib/gi18n.h>
#include <gtk/gtk.h>

enum {
  COLOR_SET,
  N_SIGNALS
};

static guint area_signals[N_SIGNALS] = { 0, };

typedef struct
{
  GtkWidget parent_instance;
  cairo_surface_t *surface;
  cairo_t *cr;
  GdkRGBA draw_color;
  GtkPadController *pad_controller;
  double brush_size;
  GtkGesture *gesture;
} DrawingArea;

typedef struct
{
  GtkWidgetClass parent_class;
} DrawingAreaClass;

static GtkPadActionEntry pad_actions[] = {
  { GTK_PAD_ACTION_BUTTON, 1, -1, N_("Black"), "pad.black" },
  { GTK_PAD_ACTION_BUTTON, 2, -1, N_("Pink"), "pad.pink" },
  { GTK_PAD_ACTION_BUTTON, 3, -1, N_("Green"), "pad.green" },
  { GTK_PAD_ACTION_BUTTON, 4, -1, N_("Red"), "pad.red" },
  { GTK_PAD_ACTION_BUTTON, 5, -1, N_("Purple"), "pad.purple" },
  { GTK_PAD_ACTION_BUTTON, 6, -1, N_("Orange"), "pad.orange" },
  { GTK_PAD_ACTION_STRIP, -1, -1, N_("Brush size"), "pad.brush_size" },
};

static const char *pad_colors[] = {
  "black",
  "pink",
  "green",
  "red",
  "purple",
  "orange"
};

static GType drawing_area_get_type (void);
G_DEFINE_TYPE (DrawingArea, drawing_area, GTK_TYPE_WIDGET)

static void drawing_area_set_color (DrawingArea   *area,
                                    const GdkRGBA *color);

static void
drawing_area_ensure_surface (DrawingArea *area,
                             int          width,
                             int          height)
{
  if (!area->surface ||
      cairo_image_surface_get_width (area->surface) != width ||
      cairo_image_surface_get_height (area->surface) != height)
    {
      cairo_surface_t *surface;

      surface = cairo_image_surface_create (CAIRO_FORMAT_ARGB32,
                                            width, height);
      if (area->surface)
        {
          cairo_t *cr;

          cr = cairo_create (surface);
          cairo_set_source_surface (cr, area->surface, 0, 0);
          cairo_paint (cr);

          cairo_surface_destroy (area->surface);
          cairo_destroy (area->cr);
          cairo_destroy (cr);
        }

      area->surface = surface;
      area->cr = cairo_create (surface);
    }
}

static void
drawing_area_size_allocate (GtkWidget *widget,
                            int        width,
                            int        height,
                            int        baseline)
{
  DrawingArea *area = (DrawingArea *) widget;

  drawing_area_ensure_surface (area, width, height);

  GTK_WIDGET_CLASS (drawing_area_parent_class)->size_allocate (widget, width, height, baseline);
}

static void
drawing_area_map (GtkWidget *widget)
{
  GTK_WIDGET_CLASS (drawing_area_parent_class)->map (widget);

  drawing_area_ensure_surface ((DrawingArea *) widget,
                               gtk_widget_get_width (widget),
                               gtk_widget_get_height (widget));
}

static void
drawing_area_unmap (GtkWidget *widget)
{
  DrawingArea *area = (DrawingArea *) widget;

  g_clear_pointer (&area->cr, cairo_destroy);
  g_clear_pointer (&area->surface, cairo_surface_destroy);

  GTK_WIDGET_CLASS (drawing_area_parent_class)->unmap (widget);
}

static void
drawing_area_snapshot (GtkWidget   *widget,
                       GtkSnapshot *snapshot)
{
  DrawingArea *area = (DrawingArea *) widget;
  int width, height;
  cairo_t *cr;

  width = gtk_widget_get_width (widget);
  height = gtk_widget_get_height (widget);

  cr = gtk_snapshot_append_cairo (snapshot, &GRAPHENE_RECT_INIT (0, 0, width, height));

  cairo_set_source_rgb (cr, 1, 1, 1);
  cairo_paint (cr);

  cairo_set_source_surface (cr, area->surface, 0, 0);
  cairo_paint (cr);

  cairo_set_source_rgb (cr, 0.6, 0.6, 0.6);
  cairo_rectangle (cr, 0, 0, width, height);
  cairo_stroke (cr);

  cairo_destroy (cr);
}

static void
on_pad_button_activate (GSimpleAction *action,
                        GVariant      *parameter,
                        DrawingArea   *area)
{
  const char *color = g_object_get_data (G_OBJECT (action), "color");
  GdkRGBA rgba;

  gdk_rgba_parse (&rgba, color);
  drawing_area_set_color (area, &rgba);
}

static void
on_pad_knob_change (GSimpleAction *action,
                    GVariant      *parameter,
                    DrawingArea   *area)
{
  double value = g_variant_get_double (parameter);

  area->brush_size = value;
}

static void
drawing_area_unroot (GtkWidget *widget)
{
  DrawingArea *area = (DrawingArea *) widget;
  GtkWidget *toplevel;

  toplevel = GTK_WIDGET (gtk_widget_get_root (widget));

  if (area->pad_controller)
    {
      gtk_widget_remove_controller (toplevel, GTK_EVENT_CONTROLLER (area->pad_controller));
      area->pad_controller = NULL;
    }

  GTK_WIDGET_CLASS (drawing_area_parent_class)->unroot (widget);
}

static void
drawing_area_root (GtkWidget *widget)
{
  DrawingArea *area = (DrawingArea *) widget;
  GSimpleActionGroup *action_group;
  GSimpleAction *action;
  GtkWidget *toplevel;
  int i;

  GTK_WIDGET_CLASS (drawing_area_parent_class)->root (widget);

  toplevel = GTK_WIDGET (gtk_widget_get_root (GTK_WIDGET (area)));

  action_group = g_simple_action_group_new ();
  area->pad_controller = gtk_pad_controller_new (G_ACTION_GROUP (action_group), NULL);

  for (i = 0; i < G_N_ELEMENTS (pad_actions); i++)
    {
      if (pad_actions[i].type == GTK_PAD_ACTION_BUTTON)
        {
          action = g_simple_action_new (pad_actions[i].action_name, NULL);
          g_object_set_data (G_OBJECT (action), "color",
                             (gpointer) pad_colors[i]);
          g_signal_connect (action, "activate",
                            G_CALLBACK (on_pad_button_activate), area);
        }
      else
        {
          action = g_simple_action_new_stateful (pad_actions[i].action_name,
                                                 G_VARIANT_TYPE_DOUBLE, NULL);
          g_signal_connect (action, "activate",
                            G_CALLBACK (on_pad_knob_change), area);
        }

      g_action_map_add_action (G_ACTION_MAP (action_group), G_ACTION (action));
      g_object_unref (action);
    }

  gtk_pad_controller_set_action_entries (area->pad_controller, pad_actions,
                                         G_N_ELEMENTS (pad_actions));

  gtk_widget_add_controller (toplevel, GTK_EVENT_CONTROLLER (area->pad_controller));
}

static void
drawing_area_class_init (DrawingAreaClass *klass)
{
  GtkWidgetClass *widget_class = GTK_WIDGET_CLASS (klass);

  widget_class->size_allocate = drawing_area_size_allocate;
  widget_class->snapshot = drawing_area_snapshot;
  widget_class->map = drawing_area_map;
  widget_class->unmap = drawing_area_unmap;
  widget_class->root = drawing_area_root;
  widget_class->unroot = drawing_area_unroot;

  area_signals[COLOR_SET] =
    g_signal_new ("color-set",
                  G_TYPE_FROM_CLASS (widget_class),
                  G_SIGNAL_RUN_FIRST,
                  0, NULL, NULL, NULL,
                  G_TYPE_NONE, 1, GDK_TYPE_RGBA);
}

static void
drawing_area_apply_stroke (DrawingArea   *area,
                           GdkDeviceTool *tool,
                           double         x,
                           double         y,
                           double         pressure)
{
  if (tool && gdk_device_tool_get_tool_type (tool) == GDK_DEVICE_TOOL_TYPE_ERASER)
    {
      cairo_set_line_width (area->cr, 10 * pressure * area->brush_size);
      cairo_set_operator (area->cr, CAIRO_OPERATOR_DEST_OUT);
    }
  else
    {
      cairo_set_line_width (area->cr, 4 * pressure * area->brush_size);
      cairo_set_operator (area->cr, CAIRO_OPERATOR_SATURATE);
    }

  cairo_set_source_rgba (area->cr, area->draw_color.red,
                         area->draw_color.green, area->draw_color.blue,
                         area->draw_color.alpha * pressure);

  cairo_line_to (area->cr, x, y);
  cairo_stroke (area->cr);
  cairo_move_to (area->cr, x, y);
}

static void
stylus_gesture_down (GtkGestureStylus *gesture,
                     double            x,
                     double            y,
                     DrawingArea      *area)
{
  cairo_new_path (area->cr);
}

static void
stylus_gesture_motion (GtkGestureStylus *gesture,
                       double            x,
                       double            y,
                       DrawingArea      *area)
{
  GdkTimeCoord *backlog;
  GdkDeviceTool *tool;
  double pressure;
  guint n_items;

  tool = gtk_gesture_stylus_get_device_tool (gesture);

  if (gtk_gesture_stylus_get_backlog (gesture, &backlog, &n_items))
    {
      guint i;

      for (i = 0; i < n_items; i++)
        {
          drawing_area_apply_stroke (area, tool,
                                     backlog[i].axes[GDK_AXIS_X],
                                     backlog[i].axes[GDK_AXIS_Y],
                                     backlog[i].flags & GDK_AXIS_FLAG_PRESSURE
                                        ? backlog[i].axes[GDK_AXIS_PRESSURE]
                                        : 1);
        }

      g_free (backlog);
    }
  else
    {
      if (!gtk_gesture_stylus_get_axis (gesture, GDK_AXIS_PRESSURE, &pressure))
        pressure = 1;

      drawing_area_apply_stroke (area, tool, x, y, pressure);
    }

  gtk_widget_queue_draw (GTK_WIDGET (area));
}

static void
drawing_area_init (DrawingArea *area)
{
  GtkGesture *gesture;

  gesture = gtk_gesture_stylus_new ();
  g_signal_connect (gesture, "down",
                    G_CALLBACK (stylus_gesture_down), area);
  g_signal_connect (gesture, "motion",
                    G_CALLBACK (stylus_gesture_motion), area);
  gtk_widget_add_controller (GTK_WIDGET (area), GTK_EVENT_CONTROLLER (gesture));

  area->draw_color = (GdkRGBA) { 0, 0, 0, 1 };
  area->brush_size = 1;

  area->gesture = gesture;
}

static GtkWidget *
drawing_area_new (void)
{
  return g_object_new (drawing_area_get_type (), NULL);
}

static void
drawing_area_set_color (DrawingArea   *area,
                        const GdkRGBA *color)
{
  if (gdk_rgba_equal (&area->draw_color, color))
    return;

  area->draw_color = *color;
  g_signal_emit (area, area_signals[COLOR_SET], 0, &area->draw_color);
}

static void
color_button_color_set (GtkColorDialogButton *button,
                        GParamSpec           *pspec,
                        DrawingArea          *draw_area)
{
  const GdkRGBA *color;

  color = gtk_color_dialog_button_get_rgba (button);
  drawing_area_set_color (draw_area, color);
}

static void
drawing_area_color_set (DrawingArea          *area,
                        GdkRGBA              *color,
                        GtkColorDialogButton *button)
{
  gtk_color_dialog_button_set_rgba (button, color);
}

static GtkGesture *
drawing_area_get_gesture (DrawingArea *area)
{
  return area->gesture;
}

GtkWidget *
do_paint (GtkWidget *toplevel)
{
  static GtkWidget *window = NULL;

  if (!window)
    {
      GtkWidget *draw_area, *headerbar, *button;

      window = gtk_window_new ();

      draw_area = drawing_area_new ();
      gtk_window_set_child (GTK_WINDOW (window), draw_area);

      headerbar = gtk_header_bar_new ();

      button = gtk_color_dialog_button_new (gtk_color_dialog_new ());
      g_signal_connect (button, "notify::rgba",
                        G_CALLBACK (color_button_color_set), draw_area);
      g_signal_connect (draw_area, "color-set",
                        G_CALLBACK (drawing_area_color_set), button);
      gtk_color_dialog_button_set_rgba (GTK_COLOR_DIALOG_BUTTON (button),
                                        &(GdkRGBA) { 0, 0, 0, 1 });

      gtk_header_bar_pack_end (GTK_HEADER_BAR (headerbar), button);

      button = gtk_check_button_new_with_label ("Stylus only");
      g_object_bind_property (button, "active",
                              drawing_area_get_gesture ((DrawingArea *)draw_area), "stylus-only",
                              G_BINDING_SYNC_CREATE);
      gtk_header_bar_pack_start (GTK_HEADER_BAR (headerbar), button);

      gtk_window_set_titlebar (GTK_WINDOW (window), headerbar);
      gtk_window_set_title (GTK_WINDOW (window), "Paint");
      g_object_add_weak_pointer (G_OBJECT (window), (gpointer *)&window);
    }

  if (!gtk_widget_get_visible (window))
    gtk_widget_set_visible (window, TRUE);
  else
    gtk_window_destroy (GTK_WINDOW (window));

  return window;
}

*/

/*
// Dialogs
#include <glib/gi18n.h>
#include <gtk/gtk.h>

G_GNUC_BEGIN_IGNORE_DEPRECATIONS

static GtkWidget *window = NULL;
static GtkWidget *entry1 = NULL;
static GtkWidget *entry2 = NULL;

static void
message_dialog_clicked (GtkButton *button,
                        gpointer   user_data)
{
  GtkWidget *dialog;
  static int i = 1;

  dialog = gtk_message_dialog_new (GTK_WINDOW (window),
                                   GTK_DIALOG_MODAL | GTK_DIALOG_DESTROY_WITH_PARENT,
                                   GTK_MESSAGE_INFO,
                                   GTK_BUTTONS_OK_CANCEL,
                                   "Test message");
  gtk_message_dialog_format_secondary_text (GTK_MESSAGE_DIALOG (dialog),
                                            ngettext ("Has been shown once", "Has been shown %d times", i), i);
  g_signal_connect (dialog, "response", G_CALLBACK (gtk_window_destroy), NULL);
  gtk_window_present (GTK_WINDOW (dialog));
  i++;
}

typedef struct {
  GtkWidget *local_entry1;
  GtkWidget *local_entry2;
  GtkWidget *global_entry1;
  GtkWidget *global_entry2;
} ResponseData;

static void
on_dialog_response (GtkDialog *dialog,
                    int        response,
                    gpointer   user_data)
{
  ResponseData *data = user_data;

  if (response == GTK_RESPONSE_OK)
    {
      gtk_editable_set_text (GTK_EDITABLE (data->global_entry1),
                             gtk_editable_get_text (GTK_EDITABLE (data->local_entry1)));
      gtk_editable_set_text (GTK_EDITABLE (data->global_entry2),
                             gtk_editable_get_text (GTK_EDITABLE (data->local_entry2)));
    }

  gtk_window_destroy (GTK_WINDOW (dialog));
}

static void
interactive_dialog_clicked (GtkButton *button,
                            gpointer   user_data)
{
  GtkWidget *content_area;
  GtkWidget *dialog;
  GtkWidget *table;
  GtkWidget *local_entry1;
  GtkWidget *local_entry2;
  GtkWidget *label;
  ResponseData *data;

  dialog = gtk_dialog_new_with_buttons ("Interactive Dialog",
                                        GTK_WINDOW (window),
                                        GTK_DIALOG_MODAL| GTK_DIALOG_DESTROY_WITH_PARENT|GTK_DIALOG_USE_HEADER_BAR,
                                        _("_OK"), GTK_RESPONSE_OK,
                                        _("_Cancel"), GTK_RESPONSE_CANCEL,
                                        NULL);

  gtk_dialog_set_default_response (GTK_DIALOG (dialog), GTK_RESPONSE_OK);

  content_area = gtk_dialog_get_content_area (GTK_DIALOG (dialog));

  table = gtk_grid_new ();
  gtk_widget_set_hexpand (table, TRUE);
  gtk_widget_set_vexpand (table, TRUE);
  gtk_widget_set_halign (table, GTK_ALIGN_CENTER);
  gtk_widget_set_valign (table, GTK_ALIGN_CENTER);
  gtk_box_append (GTK_BOX (content_area), table);
  gtk_grid_set_row_spacing (GTK_GRID (table), 6);
  gtk_grid_set_column_spacing (GTK_GRID (table), 6);

  label = gtk_label_new_with_mnemonic ("_Entry 1");
  gtk_grid_attach (GTK_GRID (table), label, 0, 0, 1, 1);
  local_entry1 = gtk_entry_new ();
  gtk_editable_set_text (GTK_EDITABLE (local_entry1), gtk_editable_get_text (GTK_EDITABLE (entry1)));
  gtk_grid_attach (GTK_GRID (table), local_entry1, 1, 0, 1, 1);
  gtk_label_set_mnemonic_widget (GTK_LABEL (label), local_entry1);

  label = gtk_label_new_with_mnemonic ("E_ntry 2");
  gtk_grid_attach (GTK_GRID (table), label, 0, 1, 1, 1);

  local_entry2 = gtk_entry_new ();
  gtk_editable_set_text (GTK_EDITABLE (local_entry2), gtk_editable_get_text (GTK_EDITABLE (entry2)));
  gtk_grid_attach (GTK_GRID (table), local_entry2, 1, 1, 1, 1);
  gtk_label_set_mnemonic_widget (GTK_LABEL (label), local_entry2);

  data = g_new (ResponseData, 1);
  data->local_entry1 = local_entry1;
  data->local_entry2 = local_entry2;
  data->global_entry1 = entry1;
  data->global_entry2 = entry2;

  g_signal_connect_data (dialog, "response",
                         G_CALLBACK (on_dialog_response),
                         data, (GClosureNotify) g_free,
                         0);

  gtk_window_present (GTK_WINDOW (dialog));
}

GtkWidget *
do_dialog (GtkWidget *do_widget)
{
  GtkWidget *vbox;
  GtkWidget *vbox2;
  GtkWidget *hbox;
  GtkWidget *button;
  GtkWidget *table;
  GtkWidget *label;

  if (!window)
    {
      window = gtk_window_new ();
      gtk_window_set_display (GTK_WINDOW (window),
                              gtk_widget_get_display (do_widget));
      gtk_window_set_title (GTK_WINDOW (window), "Dialogs");
      gtk_window_set_resizable (GTK_WINDOW (window), FALSE);
      g_object_add_weak_pointer (G_OBJECT (window), (gpointer *)&window);

      vbox = gtk_box_new (GTK_ORIENTATION_VERTICAL, 8);
      gtk_widget_set_margin_start (vbox, 8);
      gtk_widget_set_margin_end (vbox, 8);
      gtk_widget_set_margin_top (vbox, 8);
      gtk_widget_set_margin_bottom (vbox, 8);
      gtk_window_set_child (GTK_WINDOW (window), vbox);

      /* Standard message dialog */
      hbox = gtk_box_new (GTK_ORIENTATION_HORIZONTAL, 8);
      gtk_box_append (GTK_BOX (vbox), hbox);
      button = gtk_button_new_with_mnemonic ("_Message Dialog");
      g_signal_connect (button, "clicked",
                        G_CALLBACK (message_dialog_clicked), NULL);
      gtk_box_append (GTK_BOX (hbox), button);

      gtk_box_append (GTK_BOX (vbox), gtk_separator_new (GTK_ORIENTATION_HORIZONTAL));

      /* Interactive dialog*/
      hbox = gtk_box_new (GTK_ORIENTATION_HORIZONTAL, 8);
      gtk_box_append (GTK_BOX (vbox), hbox);
      vbox2 = gtk_box_new (GTK_ORIENTATION_VERTICAL, 0);

      button = gtk_button_new_with_mnemonic ("_Interactive Dialog");
      g_signal_connect (button, "clicked",
                        G_CALLBACK (interactive_dialog_clicked), NULL);
      gtk_box_append (GTK_BOX (hbox), vbox2);
      gtk_box_append (GTK_BOX (vbox2), button);

      table = gtk_grid_new ();
      gtk_grid_set_row_spacing (GTK_GRID (table), 4);
      gtk_grid_set_column_spacing (GTK_GRID (table), 4);
      gtk_box_append (GTK_BOX (hbox), table);

      label = gtk_label_new_with_mnemonic ("_Entry 1");
      gtk_grid_attach (GTK_GRID (table), label, 0, 0, 1, 1);

      entry1 = gtk_entry_new ();
      gtk_grid_attach (GTK_GRID (table), entry1, 1, 0, 1, 1);
      gtk_label_set_mnemonic_widget (GTK_LABEL (label), entry1);

      label = gtk_label_new_with_mnemonic ("E_ntry 2");
      gtk_grid_attach (GTK_GRID (table), label, 0, 1, 1, 1);

      entry2 = gtk_entry_new ();
      gtk_grid_attach (GTK_GRID (table), entry2, 1, 1, 1, 1);
    }

  if (!gtk_widget_get_visible (window))
    gtk_widget_set_visible (window, TRUE);
  else
    gtk_window_destroy (GTK_WINDOW (window));

  return window;
}

*/

/*
// Gtk4 (Vala): Get the mouse position relative to widget
private void listbox_name_button_clicked(Gtk.Button sender)
{
    var device_pointer= this.get_display().get_default_seat().get_pointer();
    GLib.return_if_fail(null != device_pointer);

    Gdk.ModifierType mask = 0;
    double x = 0, y = 0, x_new = 0, y_new = 0;
    GLib.return_if_fail(this.root.get_surface().get_device_position(device_pointer, out x, out y, out mask));
    GLib.return_if_fail(this.root.translate_coordinates(sender.parent, x, y, out x_new, out y_new));

    Gdk.Rectangle rect = { (int)x_new, (int)y_new, 0, 0, };
    this.order_context_menu.set_pointing_to(rect);
    this.order_context_menu.popup();
}
 */
