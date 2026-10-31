//! adw::Application, stylesheet loading and the offscreen screenshot dev mode.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gdk, gio, glib, graphene, gsk};

use crate::backend::{self, Cm};
use crate::window::MainWindow;

pub const APP_ID: &str = "io.github.HyprlandMod";
const STYLE: &str = include_str!("../resources/style.css");

/// Hidden developer flags (offscreen screenshots).
#[derive(Default, Debug)]
pub struct DevOptions {
    pub screenshot: Option<PathBuf>,
    pub page: Option<String>,
    pub dark: bool,
    pub search: Option<String>,
    pub sets: Vec<(String, String)>,
    pub advanced: bool,
    pub size: Option<(i32, i32)>,
    pub scale: f64,
}

/// Strip the dev flags from argv; returns them and the remaining arguments.
pub fn parse_dev_args(argv: Vec<String>) -> (DevOptions, Vec<String>) {
    let mut dev = DevOptions {
        scale: 1.0,
        ..Default::default()
    };
    let mut it = argv.into_iter();
    let mut rest: Vec<String> = it.next().into_iter().collect();
    while let Some(a) = it.next() {
        let mut val = || it.next().unwrap_or_default();
        match a.as_str() {
            "--screenshot" => dev.screenshot = Some(PathBuf::from(val())),
            "--page" => dev.page = Some(val()),
            "--dark" => dev.dark = true,
            "--search" => dev.search = Some(val()),
            "--set" => {
                let v = val();
                let (k, v) = v.split_once('=').unwrap_or((&v, ""));
                dev.sets.push((k.to_string(), v.to_string()));
            }
            "--advanced" => dev.advanced = true,
            "--size" => {
                let v = val();
                if let Some((w, h)) = v.split_once('x') {
                    dev.size = w.parse().ok().zip(h.parse().ok());
                }
            }
            "--scale" => dev.scale = val().parse().unwrap_or(1.0),
            _ => rest.push(a),
        }
    }
    (dev, rest)
}

fn load_core() -> Result<(Cm, Vec<crate::backend::Page>), String> {
    let schema = backend::load_schema(Some(&backend::data_root().join("schema")));
    let cm = backend::ConfigManager::new(backend::ManagerOptions::default())
        .map_err(|e| e.to_string())?;
    Ok((Rc::new(RefCell::new(cm)), schema))
}

fn show_about(app: &adw::Application) {
    let about = adw::AboutDialog::builder()
        .application_name("Hyprland Mod")
        .application_icon("preferences-desktop-appearance-symbolic")
        .developer_name("Hyprland Mod contributors")
        .version(env!("CARGO_PKG_VERSION"))
        .comments("Friendly settings for Hyprland")
        .license_type(gtk::License::Gpl30)
        .build();
    about.present(app.active_window().as_ref());
}

pub fn run() -> glib::ExitCode {
    let (dev, rest) = parse_dev_args(std::env::args().collect());
    if dev.screenshot.is_some() && std::env::var_os("WAYLAND_DISPLAY").is_some() {
        eprintln!(
            "--screenshot refuses to run on a real Wayland session; use GDK_BACKEND=broadway"
        );
        return glib::ExitCode::from(2);
    }
    let mut flags = gio::ApplicationFlags::empty();
    if dev.screenshot.is_some() {
        flags |= gio::ApplicationFlags::NON_UNIQUE;
    }
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(flags)
        .build();
    let dev = Rc::new(dev);

    let about = gio::SimpleAction::new("about", None);
    about.connect_activate(glib::clone!(
        #[weak]
        app,
        move |_, _| show_about(&app)
    ));
    app.add_action(&about);
    let quit = gio::SimpleAction::new("quit", None);
    quit.connect_activate(glib::clone!(
        #[weak]
        app,
        move |_, _| app.quit()
    ));
    app.add_action(&quit);
    app.set_accels_for_action("app.quit", &["<Control>q"]);

    let d = dev.clone();
    app.connect_startup(move |_| {
        let sm = adw::StyleManager::default();
        if d.screenshot.is_some() {
            // broadway without a connected browser never ticks the frame clock, so
            // transitions would freeze mid-way: render the settled state instead
            if let Some(s) = gtk::Settings::default() {
                s.set_gtk_enable_animations(false);
            }
            sm.set_color_scheme(if d.dark {
                adw::ColorScheme::ForceDark
            } else {
                adw::ColorScheme::ForceLight
            });
        } else {
            sm.set_color_scheme(adw::ColorScheme::Default);
        }
        let provider = gtk::CssProvider::new();
        provider.load_from_string(STYLE);
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    });

    let holder: Rc<RefCell<Option<MainWindow>>> = Rc::new(RefCell::new(None));
    app.connect_activate(move |app| {
        if let Some(w) = holder.borrow().as_ref() {
            w.window.present();
            return;
        }
        let loaded = load_core();
        if let Err(e) = &loaded {
            eprintln!("hyprland-mod: {e}");
        }
        let mw = MainWindow::new(app, loaded);
        if let Some((w, h)) = dev.size {
            mw.window.set_default_size(w, h);
        }
        mw.window.present();
        if dev.screenshot.is_some() {
            if let Some(st) = &mw.state {
                st.dev_prepare(
                    dev.page.as_deref(),
                    dev.search.as_deref(),
                    &dev.sets,
                    dev.advanced,
                );
            }
            let (app, dev) = (app.clone(), dev.clone());
            let window = mw.window.clone();
            let state = mw.state.clone();
            glib::timeout_add_local_once(Duration::from_millis(1400), move || {
                let tries = Rc::new(Cell::new(0));
                glib::timeout_add_local(Duration::from_millis(200), move || {
                    if let Some(st) = &state {
                        st.apply_bar.settle();
                    }
                    screenshot(&app, &window, &dev, &tries)
                });
            });
        }
        *holder.borrow_mut() = Some(mw);
    });
    app.run_with_args(&rest)
}

fn screenshot(
    app: &adw::Application,
    win: &adw::ApplicationWindow,
    dev: &DevOptions,
    tries: &Cell<u32>,
) -> glib::ControlFlow {
    let Some(target) = win.content() else {
        app.quit();
        return glib::ControlFlow::Break;
    };
    let (w, h) = (target.width(), target.height());
    if !win.is_mapped() || w <= 0 {
        eprintln!("screenshot: window not mapped");
        app.quit();
        return glib::ControlFlow::Break;
    }
    // layout only runs on frame-clock ticks, which broadway rarely sends:
    // lay out late changes ourselves at the same size
    target.allocate(w, h, -1, None);
    let Some(parent) = target.parent() else {
        app.quit();
        return glib::ControlFlow::Break;
    };
    let inner = gtk::Snapshot::new();
    parent.snapshot_child(&target, &inner);
    let Some(content) = inner.to_node() else {
        tries.set(tries.get() + 1);
        if tries.get() < 25 {
            target.queue_draw();
            return glib::ControlFlow::Continue;
        }
        eprintln!("screenshot: nothing rendered");
        app.quit();
        return glib::ControlFlow::Break;
    };
    let scale = dev.scale as f32;
    let snap = gtk::Snapshot::new();
    snap.scale(scale, scale);
    // the window background lives on the toplevel, which we don't snapshot
    let bg = if adw::StyleManager::default().is_dark() {
        "#222226"
    } else {
        "#fafafb"
    };
    snap.append_color(
        &gdk::RGBA::parse(bg).expect("color"),
        &graphene::Rect::new(0.0, 0.0, w as f32, h as f32),
    );
    snap.append_node(&content);
    let result = (|| -> Result<PathBuf, String> {
        let node = snap.to_node().ok_or("empty snapshot")?;
        let display = gdk::Display::default().ok_or("no display")?;
        let renderer = gsk::CairoRenderer::new();
        renderer
            .realize_for_display(&display)
            .map_err(|e| e.to_string())?;
        let rect = graphene::Rect::new(0.0, 0.0, w as f32 * scale, h as f32 * scale);
        let tex = renderer.render_texture(&node, Some(&rect));
        renderer.unrealize();
        let out = std::path::absolute(dev.screenshot.as_ref().ok_or("no path")?)
            .map_err(|e| e.to_string())?;
        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        tex.save_to_png(&out).map_err(|e| e.to_string())?;
        Ok(out)
    })();
    match result {
        Ok(out) => eprintln!("screenshot: {} ({w}x{h})", out.display()),
        Err(e) => eprintln!("screenshot failed: {e}"),
    }
    app.quit();
    glib::ControlFlow::Break
}
