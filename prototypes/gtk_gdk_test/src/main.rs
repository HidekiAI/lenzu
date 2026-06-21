mod dump_image;

use std::sync::OnceLock;

use gtk4::{glib, prelude::*, Box, Button, Image, Orientation, Picture};
use tokio::runtime::Runtime;

const APP_ID: &str = "tld.mydomain.lenzu.prototype.gtk_gdk_test";

fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| Runtime::new().expect("Setting up tokio runtime needs to succeed."))
}

fn main() -> glib::ExitCode {
    let application = gtk4::Application::builder().application_id(APP_ID).build();
    application.connect_activate(build_ui);
    application.run()
}

fn build_ui(application: &gtk4::Application) {
    let image_path = "./assets/ubunchu01_02.png";
    // let's verify that file actually exists using path
    let path = std::path::Path::new(image_path);
    if !path.exists() {
        println!(
            "File does not exist: {:?} (pwd: {})",
            path,
            std::env::current_dir().unwrap().display()
        );
        return;
    }

    let window = gtk4::ApplicationWindow::new(application);
    window.set_title(Some("gtk_gdk_test"));
    window.set_default_size(1024, 768);
    window.set_visible(false);
    let window_scrollable = gtk4::ScrolledWindow::new();
    window_scrollable.set_visible(true);
    window_scrollable.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Automatic);
    window.set_child(Some(&window_scrollable));

    // container to append multiple children
    let parent_box = Box::new(Orientation::Vertical, 0);

    // as Picture
    let picture = Picture::new();
    picture.set_filename(Some(image_path));
    picture.set_visible(true);
    let pic_paintable_dim = match picture.paintable() {
        Some(paintable) => (paintable.intrinsic_width(), paintable.intrinsic_height()),
        None => (0, 0),
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
    parent_box.append(&image);

    let (sender_quit_signal, receiver_quit_signal) = async_channel::bounded(1);

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
        let sender = sender_quit_signal.clone();
        runtime().spawn(async move {
            sender.send(true).await.expect("Signal channel is unopenend");
        });
        println!("Signal sent to quit...");
    });
    parent_box.append(&button_quit);
    glib::spawn_future_local(async move {
        while let Ok(quit_signaled) = receiver_quit_signal.recv().await {
            if quit_signaled {
                println!("Quitting...");
                std::process::exit(0);
            }
        }
    });

    //// let gtk4::Box hold strong ref to the button(s)
    //let my_box: Box = Box::builder().orientation(Orientation::Vertical).build();
    //my_box.append(button_quit);
    //parent_box.append(Some(&my_box));

    // all is attached to parent_box, now attach itself to window
    //window.set_child(Some(&parent_box));
    window_scrollable.set_child(Some(&parent_box));
    #[allow(deprecated)]
    window.show();
    window.set_visible(true);
    window.present(); // mark (child scene-graph nodes) for refresh
}

//  static void
//  file_opened (GObject      *source,
//               GAsyncResult *result,
//               void         *data)
//  {
//    GFile *file;
//    GError *error = NULL;
//    GdkTexture *texture;
//
//    file = gtk_file_dialog_open_finish (GTK_FILE_DIALOG (source), result, &error);
//
//    if (!file)
//      {
//        g_print ("%s\n", error->message);
//        g_error_free (error);
//        return;
//      }
//
//    texture = gdk_texture_new_from_file (file, &error);
//    g_object_unref (file);
//    if (!texture)
//      {
//        g_print ("%s\n", error->message);
//        g_error_free (error);
//        return;
//      }
//
//    g_object_set (G_OBJECT (data), "texture", texture, NULL);
//    g_object_unref (texture);
//  }
//  static void
//  open_file (GtkWidget *picker,
//             GtkWidget *demo)
//  {
//    GtkWindow *parent = GTK_WINDOW (gtk_widget_get_root (picker));
//    GtkFileDialog *dialog;
//    GtkFileFilter *filter;
//    GListStore *filters;
//
//    dialog = gtk_file_dialog_new ();
//
//    filter = gtk_file_filter_new ();
//    gtk_file_filter_set_name (filter, "Images");
//    gtk_file_filter_add_pixbuf_formats (filter);
//    filters = g_list_store_new (GTK_TYPE_FILE_FILTER);
//    g_list_store_append (filters, filter);
//    g_object_unref (filter);
//
//    gtk_file_dialog_set_filters (dialog, G_LIST_MODEL (filters));
//    g_object_unref (filters);
//
//    gtk_file_dialog_open (dialog, parent, NULL, file_opened, demo);
//
//    g_object_unref (dialog);
//  }
//  static void
//  rotate (GtkWidget *button,
//          GtkWidget *demo)
//  {
//    float angle;
//
//    g_object_get (demo, "angle", &angle, NULL);
//
//    angle = fmodf (angle + 90.f, 360.f);
//
//    g_object_set (demo, "angle", angle, NULL);
//  }
//
//  static gboolean
//  transform_to (GBinding     *binding,
//                const GValue *src,
//                GValue       *dest,
//                gpointer      user_data)
//  {
//    double from;
//    float to;
//
//    from = g_value_get_double (src);
//    to = (float) pow (2., from);
//    g_value_set_float (dest, to);
//
//    return TRUE;
//  }
//  static gboolean
//  transform_from (GBinding     *binding,
//                  const GValue *src,
//                  GValue       *dest,
//                  gpointer      user_data)
//  {
//    float to;
//    double from;
//
//    to = g_value_get_float (src);
//    from = log2 (to);
//    g_value_set_double (dest, from);
//
//    return TRUE;
//  }
//  GtkWidget *
//  do_image_scaling (GtkWidget *do_widget)
//  {
//    static GtkWidget *window = NULL;
//
//    if (!window)
//      {
//        GtkWidget *box;
//        GtkWidget *box2;
//        GtkWidget *sw;
//        GtkWidget *widget;
//        GtkWidget *scale;
//        GtkWidget *dropdown;
//        GtkWidget *button;
//
//        window = gtk_window_new ();
//        gtk_window_set_title (GTK_WINDOW (window), "Image Scaling");
//        gtk_window_set_default_size (GTK_WINDOW (window), 600, 400);
//        gtk_window_set_display (GTK_WINDOW (window),
//                                gtk_widget_get_display (do_widget));
//        g_object_add_weak_pointer (G_OBJECT (window), (gpointer *)&window);
//
//        box = gtk_box_new (GTK_ORIENTATION_VERTICAL, 0);
//        gtk_window_set_child (GTK_WINDOW (window), box);
//
//        sw = gtk_scrolled_window_new ();
//        gtk_widget_set_vexpand (sw, TRUE);
//        gtk_box_append (GTK_BOX (box), sw);
//
//        widget = demo3_widget_new ("/transparent/portland-rose.jpg");
//        gtk_scrolled_window_set_child (GTK_SCROLLED_WINDOW (sw), widget);
//
//        box2 = gtk_box_new (GTK_ORIENTATION_HORIZONTAL, 0);
//        gtk_box_append (GTK_BOX (box), box2);
//
//        button = gtk_button_new_from_icon_name ("document-open-symbolic");
//        gtk_widget_set_tooltip_text (button, "Open File");
//        g_signal_connect (button, "clicked", G_CALLBACK (open_file), widget);
//        gtk_box_append (GTK_BOX (box2), button);
//
//        button = gtk_button_new_from_icon_name ("object-rotate-right-symbolic");
//        gtk_widget_set_tooltip_text (button, "Rotate");
//        g_signal_connect (button, "clicked", G_CALLBACK (rotate), widget);
//        gtk_box_append (GTK_BOX (box2), button);
//
//        scale = gtk_scale_new_with_range (GTK_ORIENTATION_HORIZONTAL, -10., 10., 0.1);
//        gtk_scale_add_mark (GTK_SCALE (scale), 0., GTK_POS_TOP, NULL);
//        gtk_widget_set_tooltip_text (scale, "Zoom");
//        gtk_accessible_update_property (GTK_ACCESSIBLE (scale),
//                                        GTK_ACCESSIBLE_PROPERTY_LABEL, "Zoom",
//                                        -1);
//        gtk_range_set_value (GTK_RANGE (scale), 0.);
//        gtk_widget_set_hexpand (scale, TRUE);
//        gtk_box_append (GTK_BOX (box2), scale);
//
//        dropdown = gtk_drop_down_new (G_LIST_MODEL (gtk_string_list_new ((const char *[]){ "Linear", "Nearest", "Trilinear", NULL })), NULL);
//        gtk_widget_set_tooltip_text (dropdown, "Filter");
//        gtk_accessible_update_property (GTK_ACCESSIBLE (dropdown),
//                                        GTK_ACCESSIBLE_PROPERTY_LABEL, "Filter",
//                                        -1);
//        gtk_box_append (GTK_BOX (box2), dropdown);
//
//        g_object_bind_property (dropdown, "selected", widget, "filter", G_BINDING_DEFAULT);
//
//        g_object_bind_property_full (gtk_range_get_adjustment (GTK_RANGE (scale)), "value",
//                                     widget, "scale",
//                                     G_BINDING_BIDIRECTIONAL,
//                                     transform_to,
//                                     transform_from,
//                                     NULL, NULL);
//      }
//
//    if (!gtk_widget_get_visible (window))
//      gtk_widget_set_visible (window, TRUE);
//    else
//      gtk_window_destroy (GTK_WINDOW (window));
//
//    return window;
//  }
