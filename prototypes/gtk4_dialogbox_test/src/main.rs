use gtk4::{
    glib::{self, clone},
    prelude::*,
}; // Assumes that gtk4 is in the Cargo.toml file is set to features=["v4_14"] (meaning 4.10 methods such as GtkDialog is deprecated and replaced with GtkWindow)
use std::{cell::RefCell, rc::Rc, sync::OnceLock, time::SystemTime};
use tokio::runtime::Runtime;

fn tokio_runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| Runtime::new().expect("Setting up tokio runtime needs to succeed."))
}
// A test to verify the replacement of GtkDialog* and Gtk*Dialog (i.e. GtkMessageDialog)
// which were deprecated in GTK 4.10 in which the suggested method is to
// use GtkWindow directly.
fn main() -> glib::ExitCode {
    let app = gtk4::Application::new(
        Some("com.codemonkeyninja.hidekiai.lenzu.prototypes.gtk4_dialogbox_test"),
        Default::default(),
    );

    let _signal_id = app.connect_activate(build_ui);

    app.run()
}

fn build_ui(app: &gtk4::Application) {
    let app_window = gtk4::ApplicationWindow::builder()
        .application(app)
        .title("Dialog Box Test")
        .default_width(200)
        .default_height(200)
        .build();

    let status_label = Rc::new(RefCell::new(gtk4::Label::new(Some("Dialog Box Test"))));
    app_window.set_child(Some(&*status_label.clone().borrow_mut() as &gtk4::Label)); // Error: type annotations needed cannot satisfy `_: IsA<gtk4::Widget>`

    // container to append multiple children
    let parent_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

    // Create a button with label and margins
    let (sender_quit_signal, receiver_quit_signal) = async_channel::bounded(1);
    let button_quit: gtk4::Button = gtk4::Button::builder()
        .label("Quit")
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .width_request(16 * 16)
        .height_request(16)
        .halign(gtk4::Align::Start) // anchor to bottom left
        .valign(gtk4::Align::End)
        .build();
    let _signal_id = button_quit.connect_clicked(move |_| {
        println!("Signal quitting...");
        tokio_runtime().spawn(clone!(#[strong] sender_quit_signal, async move {
            sender_quit_signal.send(true).await.expect("Signal channel is unopenend");
        }));
        println!("Signal sent to quit...");
    });
    parent_box.append(&button_quit);
    glib::spawn_future_local(clone!(#[weak] button_quit, async move {
        while let Ok(quit_signaled) = receiver_quit_signal.recv().await {
            if quit_signaled {
                button_quit.set_label("Quitting...");

                // quit applications
                std::process::exit(0); // for now, brute-force quit, in future will elegantly signal for exit...
                //break;
            }
        }
    }));

    //let current_label = status_label.clone().borrow_mut().label();
    let open_dialog_button = gtk4::Button::builder()
        .label(status_label.clone().borrow_mut().label()) // unsure why I have to use borrow_mut() (instead of borrow()) here since I only want read-only MUTEX lock...
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .width_request(16 * 16)
        .height_request(16)
        .halign(gtk4::Align::End) // anchor to bottom right
        .valign(gtk4::Align::End)
        .build();
    let status_label_cloned = status_label.clone(); // clone status_label so that it'll increment the reference count
    let _signal_id = open_dialog_button.connect_clicked(move |_button_open_self| {
        let (dialog_close_signal_sender, dialog_close_signal_receiver): (
            async_channel::Sender<bool>,
            async_channel::Receiver<bool>,
        ) = async_channel::bounded(1);
        let dialogbox_rc = Rc::new(RefCell::new(gtk4::Window::new()));
        dialogbox_rc.clone().borrow_mut().set_title(Some("Dialog Box"));
        dialogbox_rc.clone().borrow_mut().set_default_size(200, 200);
        dialogbox_rc.clone().borrow_mut().set_modal(true);
        //dialogbox_rc.borrow().set_transient_for(Some(&parent_box));

        let close_button = gtk4::Button::builder()
            .label("Close Dialog")
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .width_request(16 * 16)
            .height_request(16)
            .halign(gtk4::Align::End) // anchor to bottom right
            .valign(gtk4::Align::End)
            .build();
        let _sender_rc = dialog_close_signal_sender.clone(); // increment ref-count
        // It's OK to move sender signal into the closure since it's the only sender/producer
        let _signal_id = close_button.connect_clicked(move |_close_button_self| {
            println!("Signal closing dialog...");
            // signal parent to close
            let _join_handle = tokio_runtime().spawn(clone!(#[strong] dialog_close_signal_sender, async move {
                dialog_close_signal_sender.send(true).await.expect("Failed to send close signal");
            }));
        });

        // Similar to sender, it's OK to move ownership of receiver into the closure (if needed) since it's the only receiver/consumer 
        let dbox_cloned = dialogbox_rc.clone(); // increment ref-count
        let _join_handle = glib::spawn_future_local(async move {
            while let Ok(close_signaled) = dialog_close_signal_receiver.recv().await {
                if close_signaled {
                    println!("Closing dialog...");
                    dbox_cloned.clone().borrow_mut().close();
                }
            }
        });
        dialogbox_rc.clone().borrow_mut().set_child(Some(&close_button));

        // replace glib::timeout_add_seconds() with glib::timeout_add_seconds_local().
        // The latter is the correct function to use when you want the timeout to be
        // run in the same thread as the one where this function gets called. This
        // is usually what you want. If you want the timeout to be run in the main thread,
        // use glib::timeout_add_seconds instead. But in this case, since you’re updating
        // the UI from the timeout, it needs to be run in the same thread as the one
        // where this function gets called. Otherwise, you’ll get thread safety issues.
        let start_time = SystemTime::now();
        let status_label_cloned_cloned = status_label_cloned.clone(); // clone status_label so that it'll increment the reference count
        glib::timeout_add_seconds_local(1, move || {
            let elapsed = start_time.elapsed().unwrap().as_secs();
            status_label_cloned_cloned
                .clone()
                .borrow_mut()
                .set_label(&format!("Dialog open for: {} seconds", elapsed));
            glib::ControlFlow::Continue
        });

        let status_label_cloned_cloned = status_label_cloned.clone(); // clone status_label so that it'll increment the reference count
        let _signal_id = dialogbox_rc
            .clone()
            .borrow_mut()
            .connect_close_request(move |dialog_borrowed| {
                let elapsed = start_time.elapsed().unwrap().as_secs();
                status_label_cloned_cloned
                    .clone()
                    .borrow_mut()
                    .set_label(&format!("Dialog was open for: {} seconds", elapsed));
                dialog_borrowed.close();
                gtk4::glib::signal::Propagation::Stop
            });

        dialogbox_rc.borrow_mut().present();
    }); // open_dialog_button.connect_clicked()
    parent_box.append(&open_dialog_button);

    app_window.set_child(Some(&parent_box));
    app_window.present();
} // ```
