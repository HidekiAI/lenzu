mod imp;

use gtk4::{gdk, glib};

glib::wrapper! {
    pub struct MyPaintableCanvas(ObjectSubclass<imp::MyPaintableCanvas>) @implements gdk::Paintable;
}

impl Default for MyPaintableCanvas {
    fn default() -> Self {
        glib::Object::new()
    }
}
