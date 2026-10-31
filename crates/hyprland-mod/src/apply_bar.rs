//! Floating "unsaved changes" bar that springs up from the bottom.

use std::cell::{Cell, OnceCell};

use crate::backend::WithClasses;
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib::{self, clone};

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct ApplyBar {
        pub offset: Cell<f64>,
        pub shown: Cell<bool>,
        pub card: OnceCell<gtk::Box>,
        pub label: OnceCell<gtk::Label>,
        pub discard: OnceCell<gtk::Button>,
        pub apply: OnceCell<gtk::Button>,
        pub anim_in: OnceCell<adw::SpringAnimation>,
        pub anim_out: OnceCell<adw::TimedAnimation>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ApplyBar {
        const NAME: &'static str = "HmApplyBar";
        type Type = super::ApplyBar;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_layout_manager_type::<gtk::BinLayout>();
        }
    }

    impl ObjectImpl for ApplyBar {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            self.offset.set(1.0);
            obj.set_halign(gtk::Align::Center);
            obj.set_valign(gtk::Align::End);
            obj.set_visible(false);

            let card = gtk::Box::builder()
                .spacing(6)
                .build()
                .with_classes(&["apply-bar"]);
            let icon = gtk::Image::builder()
                .icon_name("document-edit-symbolic")
                .build()
                .with_classes(&["apply-bar-icon"]);
            card.append(&icon);
            let label = gtk::Label::builder()
                .xalign(0.0)
                .build()
                .with_classes(&["apply-bar-label"]);
            card.append(&label);
            let discard = gtk::Button::builder()
                .label("Discard")
                .build()
                .with_classes(&["flat"]);
            let apply = gtk::Button::builder()
                .label("Apply")
                .tooltip_text("Apply changes (Ctrl+S)")
                .build()
                .with_classes(&["suggested-action", "pill-small"]);
            card.append(&discard);
            card.append(&apply);
            card.set_parent(&*obj);

            let target = adw::CallbackAnimationTarget::new(clone!(
                #[weak]
                obj,
                move |v| obj.set_offset(v)
            ));
            let anim_in = adw::SpringAnimation::new(
                &*obj,
                1.0,
                0.0,
                adw::SpringParams::new(0.72, 1.0, 380.0),
                target.clone(),
            );
            anim_in.set_epsilon(0.0005);
            let anim_out = adw::TimedAnimation::new(&*obj, 0.0, 1.0, 170, target);
            anim_out.set_easing(adw::Easing::EaseInCubic);
            anim_out.connect_done(clone!(
                #[weak]
                obj,
                move |_| obj.set_visible(obj.imp().shown.get())
            ));

            let _ = self.card.set(card);
            let _ = self.label.set(label);
            let _ = self.discard.set(discard);
            let _ = self.apply.set(apply);
            let _ = self.anim_in.set(anim_in);
            let _ = self.anim_out.set(anim_out);
        }

        fn dispose(&self) {
            if let Some(card) = self.card.get() {
                card.unparent();
            }
        }
    }

    impl WidgetImpl for ApplyBar {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let h = obj.height() as f32 + 24.0;
            snapshot.save();
            snapshot.translate(&gtk::graphene::Point::new(
                0.0,
                self.offset.get() as f32 * h,
            ));
            if let Some(card) = self.card.get() {
                obj.snapshot_child(card, snapshot);
            }
            snapshot.restore();
        }
    }
}

glib::wrapper! {
    pub struct ApplyBar(ObjectSubclass<imp::ApplyBar>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl ApplyBar {
    pub fn new(on_discard: impl Fn() + 'static, on_apply: impl Fn() + 'static) -> Self {
        let bar: Self = glib::Object::new();
        let imp = bar.imp();
        imp.discard
            .get()
            .expect("discard")
            .connect_clicked(move |_| on_discard());
        imp.apply
            .get()
            .expect("apply")
            .connect_clicked(move |_| on_apply());
        bar
    }

    fn set_offset(&self, v: f64) {
        self.imp().offset.set(v);
        self.set_opacity((1.0 - v * 1.2).clamp(0.0, 1.0));
        self.queue_draw();
    }

    pub fn set_count(&self, n: usize) {
        let imp = self.imp();
        let text = if n == 1 {
            "1 unsaved change".to_string()
        } else {
            format!("{n} unsaved changes")
        };
        imp.label.get().expect("label").set_label(&text);
        let show = n > 0;
        if show == imp.shown.get() {
            return;
        }
        imp.shown.set(show);
        let (anim_in, anim_out) = (
            imp.anim_in.get().expect("in"),
            imp.anim_out.get().expect("out"),
        );
        if show {
            anim_out.pause();
            self.set_visible(true);
            anim_in.set_value_from(imp.offset.get());
            anim_in.play();
        } else {
            anim_in.pause();
            anim_out.set_value_from(imp.offset.get());
            anim_out.play();
        }
    }

    /// Jump to the end state (offscreen screenshots never tick the frame clock).
    pub fn settle(&self) {
        let imp = self.imp();
        if imp.shown.get() {
            imp.anim_in.get().expect("in").skip();
        } else {
            imp.anim_out.get().expect("out").skip();
        }
    }
}
