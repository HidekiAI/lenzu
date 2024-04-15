use gtk4::{
    ffi::gtk_snapshot_render_background,
    gdk::{self, ffi::GdkRGBA},
    glib::{self, property::PropertyGet},
    graphene, gsk,
    prelude::*,
    subclass::prelude::*,
};

#[derive(Default)]
pub struct MyPaintableCanvas {}

#[glib::object_subclass]
impl ObjectSubclass for MyPaintableCanvas {
    const NAME: &'static str = "MyPaintableCanvas";
    type Type = super::MyPaintableCanvas;
    type Interfaces = (gdk::Paintable,);
}

impl ObjectImpl for MyPaintableCanvas {}

impl PaintableImpl for MyPaintableCanvas {
    fn flags(&self) -> gdk::PaintableFlags {
        // Fixed size
        gdk::PaintableFlags::SIZE
    }

    fn intrinsic_width(&self) -> i32 {
        200
    }

    fn intrinsic_height(&self) -> i32 {
        200
    }

    // render scene-graph
    fn snapshot(&self, snapshot: &gdk::Snapshot, width: f64, height: f64) {
        //let context: *mut gtk4::ffi::GtkStyleContext = snapshot.to_glib_none().0;
        //let context = snapshot.get(PropertyGet::name("context"));
        ////paintable_snapshot_texture(snapshot, None, &rect, &color);
        //unsafe {
        //    gtk_snapshot_render_background(snapshot, context,  0, 0, width, height);
        //}
    }
}
