use std::cell::{Ref, RefCell, RefMut};
use std::collections::{HashMap, HashSet};
use std::env;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use gtk4::gdk::{Display, DragAction, Key, ModifierType, Rectangle, Texture};
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::gio::{
    ApplicationFlags, Cancellable, File, MemoryInputStream, Menu, MenuItem, MenuModel,
    SimpleAction, SimpleActionGroup,
};
use gtk4::glib;
use gtk4::glib::prelude::{ObjectExt, StaticType, ToVariant};
use gtk4::glib::{closure_local, format_size, Bytes, ExitCode, Variant};
use gtk4::graphene::Point;
use gtk4::prelude::{
    ActionExt, ActionMapExt, ApplicationExt, ApplicationExtManual, BoxExt, ButtonExt, DisplayExt,
    DrawingAreaExt, EditableExt, EventControllerExt, FileExt, GtkApplicationExt, GtkWindowExt, IsA,
    NativeExt, OrientableExt, PopoverExt, SeatExt, SurfaceExt, ToggleButtonExt, WidgetExt,
};
use gtk4::{
    gdk, AlertDialog, Align, Application, ApplicationWindow, Button, CssProvider, DropTarget,
    EventControllerKey, FileDialog, FileFilter, GestureClick, HeaderBar, Image, Label, Orientation,
    Paned, Popover, PopoverMenu, PositionType, SearchEntry, Separator, Stack, ToggleButton, Widget,
    Window,
};
use pangocairo::glib::Propagation;
use pangocairo::pango::EllipsizeMode;
use resvg::{tiny_skia, usvg};

use crate::annotations::{
    export_book, Anchor, Annotation, AnnotationColor, AnnotationStatus, AnnotationStore, BookId,
    BookMetadata, NewAnnotation,
};
use crate::book::{Book, Line};
use crate::color::Color32;
use crate::common::{txt_lines, Position};
use crate::config::{BookLoadingInfo, Configuration, ReadingInfo, SidebarPosition};
use crate::container::{load_book, load_container, BookContent, BookName, Container};
use crate::controller::Controller;
use crate::gui::chapter_list::ChapterList;
use crate::gui::dict::{DictionaryBook, DictionaryManager};
use crate::gui::find_list::FindList;
pub use crate::gui::font::HtmlFonts;
use crate::gui::font::UserFonts;
use crate::gui::history::HistoryList;
use crate::gui::render::{PersistentHighlight, RenderContext};
use crate::gui::settings::Settings;
use crate::gui::view::{update_mouse_pointer, GuiView};
use crate::library::{collect_file_documents, LibraryDocument, LibraryIndex, SearchHit};
use crate::open::Opener;
use crate::{package_name, Asset, I18n};

mod annotations;
mod chapter_list;
mod dialogs;
mod dict;
mod find_list;
mod font;
mod history;
mod library_search;
mod math;
mod render;
mod settings;
mod view;

const MODIFIER_NONE: ModifierType = ModifierType::empty();
const MODIFIER_CTRL_SHIFT: ModifierType =
    ModifierType::CONTROL_MASK.union(ModifierType::SHIFT_MASK);

const APP_ID: &str = "net.lzrj.tbrplus";
const ICON_SIZE: i32 = 32;
const INLINE_ICON_SIZE: i32 = 16;
const MIN_FONT_SIZE: u8 = 20;
const MAX_FONT_SIZE: u8 = 50;
const FONT_FILE_EXTENSIONS: [&str; 3] = ["ttf", "otf", "ttc"];
const DICT_FILE_EXTENSIONS: [&str; 1] = ["ifo"];
const SIDEBAR_CHAPTER_LIST_NAME: &str = "chapter_list";
const SIDEBAR_DICT_NAME: &str = "dictionary_list";
const SIDEBAR_FIND_NAME: &str = "find_list";
const SIDEBAR_ANNOTATIONS_NAME: &str = "annotations";
const SIDEBAR_LIBRARY_SEARCH_NAME: &str = "library_search";

const OPEN_FILE_KEY: &str = "file-open";
const OPEN_FOLDER_KEY: &str = "folder-open";
const HISTORY_KEY: &str = "history";
const RELOAD_KEY: &str = "reload";
const BOOK_INFO_KEY: &str = "book-info";
const SIDEBAR_KEY: &str = "sidebar";
const THEME_KEY: &str = "dark-theme";
const CUSTOM_COLOR_KEY: &str = "with-custom-color";
const CUSTOM_FONT_KEY: &str = "with-custom-font";
const CUSTOM_STYLE_KEY: &str = "custom-style";
const SETTINGS_KEY: &str = "settings-dialog";

const COPY_CONTENT_KEY: &str = "copy-content";
const DICT_LOOKUP_KEY: &str = "lookup-dictionary";
const ADD_ANNOTATION_KEY: &str = "add-annotation";

const README_TEXT_FILENAME: &str = "readme";

type GuiController = Controller<RenderContext, GuiView>;
type IconMap = HashMap<String, Texture>;

#[derive(Clone, Copy)]
struct AnnotationSelection {
    from_line: usize,
    from_offset: usize,
    to_line: usize,
    to_offset: usize,
}

#[derive(Clone)]
struct LibraryJump {
    filename: String,
    inner_book: usize,
    chapter_index: usize,
    line: usize,
}

thread_local! {
    static PENDING_LIBRARY_JUMP: RefCell<Option<LibraryJump>> = RefCell::new(None);
}

struct ReadmeContainer {
    text: String,
}

impl ReadmeContainer {
    #[inline]
    fn new(text: &str) -> Self {
        ReadmeContainer {
            text: text.to_string(),
        }
    }
}

impl Container for ReadmeContainer {
    #[inline]
    fn filename(&self) -> &str {
        README_TEXT_FILENAME
    }

    #[inline]
    fn inner_book_names(&self) -> Option<&Vec<BookName>> {
        None
    }

    #[inline]
    fn book_content(&mut self, _inner_index: usize) -> Result<BookContent> {
        Ok(BookContent::Buf(self.text.as_bytes().to_vec()))
    }

    fn book_name(&self, _inner_index: usize) -> &str {
        "The e-book reader"
    }
}

struct ReadmeBook {
    lines: Vec<Line>,
}

impl ReadmeBook {
    #[inline]
    fn new(text: &str) -> Self {
        ReadmeBook {
            lines: txt_lines(text),
        }
    }
}

impl Book for ReadmeBook {
    #[inline]
    fn lines(&self) -> &Vec<Line> {
        &self.lines
    }
}

fn load_image(bytes: &[u8]) -> Option<Pixbuf> {
    let bytes = Bytes::from(bytes);
    let stream = MemoryInputStream::from_bytes(&bytes);
    let image = Pixbuf::from_stream(&stream, None::<&Cancellable>).ok()?;
    Some(image)
}

fn custom_settings(
    book: &dyn Book,
    reading: &ReadingInfo,
) -> (Option<bool>, Option<bool>, Option<Option<String>>) {
    let custom_color = if book.color_customizable() {
        Some(reading.custom_color)
    } else {
        None
    };
    let custom_font = if book.fonts_customizable() {
        Some(reading.custom_font)
    } else {
        None
    };
    let custom_style = if book.style_customizable() {
        Some(reading.custom_style.clone())
    } else {
        None
    };
    (custom_color, custom_font, custom_style)
}

fn build_ui(
    app: &Application,
    current: Option<String>,
    cfg: Rc<RefCell<Configuration>>,
    gcs: &Rc<RefCell<Vec<GuiContext>>>,
) -> Result<Option<GuiContext>> {
    let configuration = cfg.borrow_mut();
    let mut gui_contexts = gcs.borrow_mut();
    let (loading, gc_idx) = if let Some(current) = &current {
        let current = configuration.reading(current)?;
        let filename = current.filename();
        match get_gc(&gui_contexts, filename) {
            Ok(idx) => {
                gui_contexts[idx].window.present();
                return Ok(None);
            }
            Err(idx) => (Some(current), idx),
        }
    } else {
        // start tbr without filename
        if gui_contexts.is_empty() {
            (None, 0)
        } else {
            return Ok(None);
        }
    };

    let colors = configuration.curr_theme().clone();
    let (i18n, icons, fonts, db, css_provider) = if let Some(gc) = gui_contexts.get(0) {
        (
            gc.i18n.clone(),
            gc.icons.clone(),
            gc.fonts.clone(),
            gc.db.clone(),
            gc.css_provider.clone(),
        )
    } else {
        let i18n = I18n::new(&configuration.gui.lang)?;
        let i18n = Rc::new(i18n);
        let icons = load_icons();
        let icons = Rc::new(icons);
        let fonts = font::user_fonts(&configuration.gui.fonts)?;
        let fonts = Rc::new(fonts);
        let db = DictionaryBook::load(
            &configuration.gui.dictionaries,
            configuration.gui.cache_dict,
        );
        let db = Rc::new(RefCell::new(db));
        let css_provider = view::init_css(&colors);
        (i18n, icons, fonts, db, css_provider)
    };

    let container_manager = Default::default();
    let (container, book, reading) = if let Some(loading) = loading {
        let mut container = load_container(&container_manager, loading.filename())?;
        let (book, reading) = load_book(&container_manager, &mut container, loading)?;
        (container, book, reading)
    } else {
        let readme = i18n.msg("readme");
        let container: Box<dyn Container> = Box::new(ReadmeContainer::new(readme.as_ref()));
        let book: Box<dyn Book> = Box::new(ReadmeBook::new(readme.as_ref()));
        (container, book, ReadingInfo::fake(README_TEXT_FILENAME))
    };

    let mut render_context = RenderContext::new(
        colors,
        reading.font_size,
        reading.custom_color,
        reading.custom_font,
        book.leading_space(),
        configuration.gui.strip_empty_lines,
        configuration.gui.ignore_font_weight,
    );
    let view = GuiView::new(
        "main",
        configuration.render_han,
        book.custom_fonts(),
        fonts.clone(),
        &mut render_context,
    );
    let (dm, dict_view, lookup_entry) = DictionaryManager::new(
        db.clone(),
        &configuration.gui.dictionaries,
        configuration.gui.cache_dict,
        configuration.gui.dict_font_size,
        fonts.clone(),
        &i18n,
        &icons,
    );

    let dark_theme = configuration.dark_theme;
    drop(configuration);

    let (custom_color, custom_font, custom_style) = custom_settings(book.as_ref(), &reading);
    let controller = Controller::from_data(
        reading,
        container_manager,
        container,
        book,
        Box::new(view.clone()),
        &mut render_context,
    );

    let ctx = Rc::new(RefCell::new(render_context));
    let ctrl = Rc::new(RefCell::new(controller));
    let settings = Settings::new(gcs.clone());
    let (gc, chapter_list_view, find_list_view, annotations_view, library_search_view, find_entry) =
        GuiContext::new(
            app,
            settings,
            current,
            &cfg,
            &ctrl,
            &ctx,
            db,
            dm,
            icons,
            i18n.clone(),
            fonts,
            css_provider,
        );

    // now setup ui
    setup_sidebar(
        &gc,
        &view,
        &dict_view,
        chapter_list_view,
        &find_list_view,
        annotations_view,
        library_search_view,
    );
    setup_view(&gc, &view);
    setup_chapter_list(&gc);
    setup_find_list(&gc);
    if let Err(err) = gc.refresh_annotations() {
        gc.error(&err.to_string());
    }

    let (toolbar, search_box) = setup_toolbar(
        &gc,
        &view,
        &lookup_entry,
        &find_entry,
        dark_theme,
        custom_color,
        custom_font,
        custom_style,
    );

    {
        let gc = gc.clone();
        search_box.connect_activate(move |entry| {
            let search_pattern = entry.text();
            handle(&gc, |controller, render_context| {
                controller.search(&search_pattern, render_context)?;
                controller.render.grab_focus();
                Ok(())
            });
        });
        let view = view.clone();
        search_box.connect_stop_search(move |_| {
            view.grab_focus();
        });
    }
    {
        let gc = gc.clone();
        let view_for_shortcuts = view.clone();
        let key_event = EventControllerKey::new();
        key_event.connect_key_pressed(move |_, key, _, modifier| {
            let (key, modifier) = ignore_cap(key, modifier);
            match (key, modifier) {
                (Key::space | Key::Page_Down, MODIFIER_NONE) => {
                    handle(&gc, |controller, render_context| {
                        controller.next_page(render_context)
                    });
                    Propagation::Stop
                }
                (Key::space, ModifierType::SHIFT_MASK) | (Key::Page_Up, MODIFIER_NONE) => {
                    handle(&gc, |controller, render_context| {
                        controller.prev_page(render_context)
                    });
                    Propagation::Stop
                }
                (Key::Home, MODIFIER_NONE) => {
                    apply(&gc, |controller, render_context| {
                        controller.redraw_at(0, 0, render_context)
                    });
                    Propagation::Stop
                }
                (Key::End, MODIFIER_NONE) => {
                    apply(&gc, |controller, render_context| {
                        controller.goto_end(render_context)
                    });
                    Propagation::Stop
                }
                (Key::Down, MODIFIER_NONE) => {
                    handle(&gc, |controller, render_context| {
                        controller.step_next(render_context)
                    });
                    Propagation::Stop
                }
                (Key::Up, MODIFIER_NONE) => {
                    handle(&gc, |controller, render_context| {
                        controller.step_prev(render_context)
                    });
                    Propagation::Stop
                }
                (Key::n, MODIFIER_NONE) => {
                    handle(&gc, |controller, render_context| {
                        controller.search_again(true, render_context)
                    });
                    Propagation::Stop
                }
                (Key::N, ModifierType::SHIFT_MASK) => {
                    handle(&gc, |controller, render_context| {
                        controller.search_again(false, render_context)
                    });
                    Propagation::Stop
                }
                (Key::d, ModifierType::CONTROL_MASK) => {
                    handle(&gc, |controller, render_context| {
                        controller.switch_toc(true, render_context)
                    });
                    Propagation::Stop
                }
                (Key::b, ModifierType::CONTROL_MASK) => {
                    handle(&gc, |controller, render_context| {
                        controller.switch_toc(false, render_context)
                    });
                    Propagation::Stop
                }
                (Key::Right, MODIFIER_NONE) => {
                    handle(&gc, |controller, render_context| {
                        controller.goto_trace(false, render_context)
                    });
                    Propagation::Stop
                }
                (Key::Left, MODIFIER_NONE) => {
                    handle(&gc, |controller, render_context| {
                        controller.goto_trace(true, render_context)
                    });
                    Propagation::Stop
                }
                (Key::Tab, MODIFIER_NONE) => {
                    apply(&gc, |controller, render_context| {
                        controller.switch_link_next(render_context)
                    });
                    Propagation::Stop
                }
                (Key::Tab, ModifierType::SHIFT_MASK) => {
                    apply(&gc, |controller, render_context| {
                        controller.switch_link_prev(render_context)
                    });
                    Propagation::Stop
                }
                (Key::Return, MODIFIER_NONE) => {
                    handle(&gc, |controller, render_context| {
                        controller.try_goto_link(render_context)
                    });
                    Propagation::Stop
                }
                (Key::equal, ModifierType::CONTROL_MASK) => {
                    apply(&gc, |controller, render_context| {
                        let reading = &mut controller.reading;
                        if reading.font_size < MAX_FONT_SIZE {
                            reading.font_size += 2;
                            controller.render.set_font_size(
                                reading.font_size,
                                controller.book.custom_fonts(),
                                render_context,
                            );
                            controller.redraw(render_context);
                        }
                    });
                    Propagation::Stop
                }
                (Key::minus, ModifierType::CONTROL_MASK) => {
                    apply(&gc, |controller, render_context| {
                        let reading = &mut controller.reading;
                        if reading.font_size > MIN_FONT_SIZE {
                            reading.font_size -= 2;
                            controller.render.set_font_size(
                                reading.font_size,
                                controller.book.custom_fonts(),
                                render_context,
                            );
                            controller.redraw(render_context);
                        }
                    });
                    Propagation::Stop
                }
                (Key::c, ModifierType::CONTROL_MASK) => {
                    copy_selection(&ctrl.borrow());
                    Propagation::Stop
                }
                (Key::k, ModifierType::CONTROL_MASK) => {
                    switch_stack(SIDEBAR_DICT_NAME, &gc, false);
                    gc.dm().focus_lookup();
                    Propagation::Stop
                }
                (Key::a, MODIFIER_CTRL_SHIFT) => {
                    switch_stack(SIDEBAR_ANNOTATIONS_NAME, &gc, false);
                    Propagation::Stop
                }
                (Key::h, MODIFIER_CTRL_SHIFT) => {
                    gc.show_annotation_composer(&view_for_shortcuts);
                    Propagation::Stop
                }
                _ => {
                    // println!("view, key: {key}, modifier: {modifier}");
                    Propagation::Proceed
                }
            }
        });
        view.add_controller(key_event);
    }

    setup_window(&gc, toolbar, view, search_box, find_entry);

    {
        let gcs = gcs.clone();
        gc.window.connect_close_request(move |win| {
            gcs.borrow_mut().retain(|c| c.window != *win);
            Propagation::Proceed
        });
    }

    gui_contexts.insert(gc_idx, gc.clone());
    Ok(Some(gc))
}

#[inline]
fn copy_selection(ctrl: &GuiController) {
    if let Some(selected_text) = ctrl.selected() {
        copy_to_clipboard(selected_text);
    }
}

#[inline]
fn copy_to_clipboard(selected_text: &str) {
    if let Some(display) = Display::default() {
        display.clipboard().set_text(selected_text);
    }
}

#[inline]
fn lookup_selection(gc: &GuiContext) {
    if let Some(selected_text) = gc.ctrl().selected() {
        gc.dm_mut().set_lookup(selected_text.to_owned());
    }
}

#[inline]
fn apply<F>(gc: &GuiContext, f: F)
where
    F: FnOnce(&mut GuiController, &mut RenderContext),
{
    let mut controller = gc.ctrl_mut();
    let orig_inner_book = controller.reading.inner_book;
    f(&mut controller, &mut gc.ctx_mut());
    let msg = controller.status().to_string();
    drop(controller);
    gc.update(&msg, ChapterListSyncMode::ReloadIfNeeded(orig_inner_book));
}

#[inline]
fn handle<T, F>(gc: &GuiContext, f: F)
where
    F: FnOnce(&mut GuiController, &mut RenderContext) -> Result<T>,
{
    let (orig_inner_book, result) = {
        let mut controller = gc.ctrl_mut();
        let orig_inner_book = controller.reading.inner_book;
        let result = f(&mut controller, &mut gc.ctx_mut());
        (orig_inner_book, result)
    };
    match result {
        Ok(_) => {
            let controller = gc.ctrl();
            let msg = controller.status().to_string();
            drop(controller);
            gc.update(&msg, ChapterListSyncMode::ReloadIfNeeded(orig_inner_book));
        }
        Err(err) => gc.error(&err.to_string()),
    }
}

fn load_icons() -> IconMap {
    const ICONS_PREFIX: &str = "gui/image/";
    let mut map = HashMap::new();
    let opt = usvg::Options::default();
    let font_db = fontdb::Database::new();
    for file in Asset::iter() {
        if file.starts_with(ICONS_PREFIX) && file.ends_with(".svg") {
            let content = Asset::get(file.as_ref()).unwrap().data;
            let tree = usvg::Tree::from_data(&content, &opt, &font_db).unwrap();
            let pixmap_size = tree.size().to_int_size();
            let mut pixmap =
                tiny_skia::Pixmap::new(pixmap_size.width(), pixmap_size.height()).unwrap();
            resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
            let png = pixmap.encode_png().unwrap();
            let bytes = Bytes::from(&png);
            let mis = MemoryInputStream::from_bytes(&bytes);
            let pixbuf = Pixbuf::from_stream(&mis, None::<&Cancellable>).unwrap();
            let name = &file[ICONS_PREFIX.len()..];
            map.insert(name.to_string(), Texture::for_pixbuf(&pixbuf));
        }
    }
    map
}

fn setup_popup_menu(gc: &GuiContext, view: &GuiView) -> PopoverMenu {
    let action_group = SimpleActionGroup::new();
    let menu = Menu::new();
    let i18n = &gc.i18n;

    view.insert_action_group("popup", Some(&action_group));

    let copy_action = SimpleAction::new(COPY_CONTENT_KEY, None);
    {
        let gc = gc.clone();
        copy_action.connect_activate(move |_, _| {
            let ctrl = gc.ctrl();
            copy_selection(&ctrl);
        });
    }
    action_group.add_action(&copy_action);
    let title = i18n.msg(COPY_CONTENT_KEY);
    let action_name = format!("popup.{}", COPY_CONTENT_KEY);
    menu.append(Some(&title), Some(&action_name));

    let lookup_action = SimpleAction::new(DICT_LOOKUP_KEY, None);
    {
        let gc = gc.clone();
        lookup_action.connect_activate(move |_, _| {
            switch_stack(SIDEBAR_DICT_NAME, &gc, false);
            lookup_selection(&gc);
        });
    }
    action_group.add_action(&lookup_action);
    let title = i18n.msg(DICT_LOOKUP_KEY);
    let menu_action_name = format!("popup.{}", DICT_LOOKUP_KEY);
    menu.append(Some(&title), Some(&menu_action_name));

    let annotation_action = SimpleAction::new(ADD_ANNOTATION_KEY, None);
    {
        let gc = gc.clone();
        let view = view.clone();
        annotation_action.connect_activate(move |_, _| gc.show_annotation_composer(&view));
    }
    action_group.add_action(&annotation_action);
    let title = i18n.msg(ADD_ANNOTATION_KEY);
    let menu_action_name = format!("popup.{}", ADD_ANNOTATION_KEY);
    menu.append(Some(&title), Some(&menu_action_name));

    let pm = PopoverMenu::builder()
        .has_arrow(false)
        .position(PositionType::Bottom)
        .menu_model(&MenuModel::from(menu))
        .build();
    pm.set_parent(view);
    pm
}

fn setup_view(gc: &GuiContext, view: &GuiView) {
    #[inline]
    fn select_text(
        gc: &GuiContext,
        from_line: usize,
        from_offset: usize,
        to_line: usize,
        to_offset: usize,
        done: bool,
    ) {
        let from = Position::new(from_line, from_offset);
        let to = Position::new(to_line, to_offset);
        gc.ctrl_mut().select_text(from, to, &mut gc.ctx_mut());
        if done {
            gc.annotation_selection.replace(Some(AnnotationSelection {
                from_line,
                from_offset,
                to_line,
                to_offset,
            }));
            if let Some(selected_text) = gc.ctrl().selected() {
                if let Some(current_tab) = gc.sidebar_stack.visible_child_name() {
                    if current_tab == SIDEBAR_DICT_NAME {
                        gc.dm_mut().set_lookup(selected_text.to_owned());
                    }
                }
            }
        }
    }

    #[inline]
    fn view_image(
        controller: &GuiController,
        line: usize,
        offset: usize,
        opener: &mut Opener,
    ) -> Result<()> {
        if let Some(line) = controller.book.lines().get(line) {
            if let Some(image) = line.image_at(offset) {
                let href = image.href();
                if let Some(image_data) = controller.book.image(href) {
                    opener.open_image(href, image_data.bytes())?;
                }
            }
        }
        Ok(())
    }

    #[inline]
    fn open_link(
        controller: &GuiController,
        line: usize,
        link_index: usize,
        opener: &mut Opener,
    ) -> Result<()> {
        if let Some(line) = controller.book.lines().get(line) {
            if let Some(link) = line.link_at(link_index) {
                opener.open_link(link.target)?;
            }
        }
        Ok(())
    }

    view.setup_gesture();
    {
        let gc = gc.clone();
        view.connect_resize(move |view, width, height| {
            let mut render_context = gc.ctx_mut();
            let mut controller = gc.ctrl_mut();
            view.resized(width, height, &mut render_context);
            controller.redraw(&mut render_context);
        });
    }

    {
        // right click
        let right_click = GestureClick::builder()
            .button(gdk::BUTTON_SECONDARY)
            .build();
        let popup_menu = setup_popup_menu(gc, view);
        let gc = gc.clone();
        right_click.connect_pressed(move |_, _, x, y| {
            if gc.ctrl().has_selection() {
                popup_menu.popup();
                let (_, width, _, _) = popup_menu.measure(Orientation::Horizontal, -1);
                let x = x as i32 + width / 2;
                popup_menu.set_pointing_to(Some(&Rectangle::new(x, y as i32, -1, -1)));
            }
        });
        view.add_controller(right_click);
    }

    {
        // open link signal
        let gc = gc.clone();
        view.connect_closure(
            GuiView::OPEN_LINK_SIGNAL,
            false,
            closure_local!(move |_: GuiView, line: u64, link_index: u64| {
                handle(&gc, |controller, render_context| {
                    controller.goto_link(line as usize, link_index as usize, render_context)
                });
            }),
        );
    }

    {
        // open image signal
        let gc = gc.clone();
        view.connect_closure(
            GuiView::OPEN_IMAGE_EXTERNAL_SIGNAL,
            false,
            closure_local!(move |_: GuiView, line: u64, offset: u64| {
                handle(&gc, |controller, _render_context| {
                    view_image(controller, line as usize, offset as usize, &mut gc.opener())
                })
            }),
        );
    }

    {
        // open link external signal
        let gc = gc.clone();
        view.connect_closure(
            GuiView::OPEN_LINK_EXTERNAL_SIGNAL,
            false,
            closure_local!(move |_: GuiView, line: u64, link_index: u64| {
                handle(&gc, |controller, _render_context| {
                    open_link(
                        controller,
                        line as usize,
                        link_index as usize,
                        &mut gc.opener(),
                    )
                })
            }),
        );
    }

    // select text signal
    {
        let gc = gc.clone();
        view.connect_closure(
            GuiView::SELECTING_TEXT_SIGNAL,
            false,
            closure_local!(move |_: GuiView,
                                 from_line: u64,
                                 from_offset: u64,
                                 to_line: u64,
                                 to_offset: u64| {
                select_text(
                    &gc,
                    from_line as usize,
                    from_offset as usize,
                    to_line as usize,
                    to_offset as usize,
                    false,
                );
            }),
        );
    }

    // text selected signal
    {
        let gc = gc.clone();
        view.connect_closure(
            GuiView::TEXT_SELECTED_SIGNAL,
            false,
            closure_local!(move |_: GuiView,
                                 from_line: u64,
                                 from_offset: u64,
                                 to_line: u64,
                                 to_offset: u64| {
                select_text(
                    &gc,
                    from_line as usize,
                    from_offset as usize,
                    to_line as usize,
                    to_offset as usize,
                    true,
                );
            }),
        );
    }

    {
        // clear selection signal
        let gc = gc.clone();
        view.connect_closure(
            GuiView::CLEAR_SELECTION_SIGNAL,
            false,
            closure_local!(move |_: GuiView| {
                gc.ctrl_mut().clear_highlight(&mut gc.ctx_mut());
            }),
        );
    }

    {
        // select word signal
        let gc = gc.clone();
        view.connect_closure(
            GuiView::SELECT_WORD_SIGNAL,
            false,
            closure_local!(move |_: GuiView, line: u64, offset: u64| {
                let line_no = line as usize;
                let controller = gc.ctrl();
                if let Some(line) = controller.book.lines().get(line_no) {
                    if let Some((from, to)) = if gc.cfg().gui.select_by_dictionary {
                        gc.db.borrow_mut().lookup_at_offset(line, offset as usize)
                    } else {
                        line.word_at_offset(offset as usize)
                    } {
                        drop(controller);
                        select_text(&gc, line_no, from, line_no, to, true);
                    }
                };
            }),
        );
    }

    {
        // show title
        let gc = gc.clone();
        view.connect_closure(
            GuiView::SHOW_TITLE_SIGNAL,
            false,
            closure_local!(move |view: GuiView, show: bool, line: u64, offset: u64| {
                if show {
                    let line_no = line as usize;
                    let controller = gc.ctrl();
                    if let Some(line) = controller.book.lines().get(line_no) {
                        let render_context = gc.ctx();
                        let char_style =
                            line.char_style_at(offset as usize, false, &render_context.colors);
                        if let Some(title) = char_style.title {
                            view.set_tooltip_text(Some(title));
                            return;
                        }
                    };
                }
                view.set_tooltip_text(None);
            }),
        );
    }

    {
        // scroll signal
        let gc = gc.clone();
        view.connect_closure(
            GuiView::SCROLL_SIGNAL,
            false,
            closure_local!(move |_: GuiView, delta: i32| {
                if delta > 0 {
                    if gc.cfg().gui.scroll_for_page {
                        handle(&gc, |controller, render_context| {
                            controller.next_page(render_context)
                        });
                    } else {
                        handle(&gc, |controller, render_context| {
                            controller.step_next(render_context)
                        });
                    }
                } else {
                    if gc.cfg().gui.scroll_for_page {
                        handle(&gc, |controller, render_context| {
                            controller.prev_page(render_context)
                        });
                    } else {
                        handle(&gc, |controller, render_context| {
                            controller.step_prev(render_context)
                        });
                    }
                }
            }),
        );
    }
}

fn setup_sidebar(
    gc: &GuiContext,
    view: &GuiView,
    dict_view: &gtk4::Box,
    chapter_list_view: gtk4::Box,
    find_list_view: &gtk4::Box,
    annotations_view: gtk4::Box,
    library_search_view: gtk4::Box,
) {
    let i18n = &gc.i18n;
    let stack = &gc.sidebar_stack;
    stack.add_titled(
        &chapter_list_view,
        Some(SIDEBAR_CHAPTER_LIST_NAME),
        &i18n.msg("tab-chapter"),
    );
    stack.add_titled(
        dict_view,
        Some(SIDEBAR_DICT_NAME),
        &i18n.msg("tab-dictionary"),
    );
    stack.add_titled(
        find_list_view,
        Some(SIDEBAR_FIND_NAME),
        &i18n.msg("tab-find"),
    );
    stack.add_titled(
        &annotations_view,
        Some(SIDEBAR_ANNOTATIONS_NAME),
        &i18n.msg("tab-annotations"),
    );
    stack.add_titled(
        &library_search_view,
        Some(SIDEBAR_LIBRARY_SEARCH_NAME),
        &i18n.msg("tab-library-search"),
    );
    stack.set_visible_child(&chapter_list_view);

    let sidebar_tab_switch = gtk4::StackSwitcher::builder().stack(&stack).build();
    let sidebar = gtk4::Box::new(Orientation::Vertical, 0);
    sidebar.append(&sidebar_tab_switch);
    sidebar.append(&gc.sidebar_stack);

    let sidebar_position = &gc.cfg().gui.sidebar_position;
    set_sidebar_position(gc, sidebar_position);

    let paned = &gc.paned;
    paned.set_start_child(Some(&sidebar));
    paned.set_end_child(Some(view));
    paned.set_position(0);

    let gc = gc.clone();
    paned.connect_position_notify(move |paned| {
        let position = paned.position();
        if position > 0 {
            sidebar_updated(&mut gc.cfg_mut(), &mut gc.dm_mut(), position)
        }
    });
}

fn sidebar_updated(
    configuration: &mut Configuration,
    dictionary_manager: &mut DictionaryManager,
    position: i32,
) {
    configuration.gui.sidebar_size = position as u32;
    dictionary_manager.resize(position, None);
}

#[inline]
fn set_sidebar_position(gc: &GuiContext, position: &SidebarPosition) {
    gc.paned.set_orientation(position.paned_orientation());
}

fn setup_chapter_list(gc1: &GuiContext) {
    {
        let gc = gc1.clone();
        gc1.chapter_list.handle_item_click(move |is_book, index| {
            let mut controller = gc.ctrl_mut();
            let mut render_context = gc.ctx_mut();
            if is_book {
                let (error, msg) = match controller.switch_book(index, &mut render_context) {
                    Ok(msg) => {
                        gc.find_list.set_inner_book(index);
                        (false, msg)
                    }
                    Err(e) => (true, e.to_string()),
                };
                update_title(&gc.window, &controller);
                update_status(error, &msg, &gc.status_bar);
            } else if let Some(msg) = controller.goto_toc(index, &mut render_context) {
                update_status(false, &msg, &gc.status_bar);
            }
        });
    }
    {
        let gc = gc1.clone();
        gc1.chapter_list.handle_cancel(move |empty| {
            // when search entry has no text, empty is true
            if empty {
                gc.toggle_sidebar();
            } else {
                gc.ctrl().render.grab_focus();
            }
        });
    }
}

fn setup_find_list(gc1: &GuiContext) {
    let gc = gc1.clone();
    gc1.find_list.set_callback(move |found_entry| {
        let mut controller = gc.ctrl_mut();
        let mut render_context = gc.ctx_mut();
        match controller.goto(
            found_entry.inner_book,
            found_entry.chapter,
            found_entry.line,
            found_entry.range.start,
            Some(found_entry.range.clone()),
            &mut render_context,
        ) {
            Ok(msg) => {
                update_title(&gc.window, &controller);
                update_status(false, &msg, &gc.status_bar);
                true
            }
            Err(e) => {
                update_status(true, &e.to_string(), &gc.status_bar);
                false
            }
        }
    })
}

fn switch_stack(tab_name: &str, gc: &GuiContext, toggle: bool) -> bool {
    let paned = &gc.paned;
    let stack = &gc.sidebar_stack;
    if paned.position() == 0 {
        stack.set_visible_child_name(tab_name);
        gc.toggle_sidebar();
        true
    } else {
        match stack.visible_child_name() {
            Some(current_tab_name) => {
                if current_tab_name == tab_name {
                    if toggle {
                        gc.toggle_sidebar();
                        false
                    } else {
                        true
                    }
                } else {
                    stack.set_visible_child_name(tab_name);
                    true
                }
            }
            _ => {
                stack.set_visible_child_name(tab_name);
                true
            }
        }
    }
}

#[inline]
fn setup_window(
    gc: &GuiContext,
    toolbar: gtk4::Box,
    view: GuiView,
    search_box: SearchEntry,
    find_entry: SearchEntry,
) {
    let header_bar = HeaderBar::new();
    header_bar.set_height_request(32);
    header_bar.pack_start(&toolbar);
    header_bar.pack_end(&gc.status_bar);
    let window = &gc.window;
    window.set_titlebar(Some(&header_bar));
    window.set_child(Some(&gc.paned));
    window.set_default_widget(Some(&view));
    window.set_focus(Some(&view));
    window.add_css_class("main-window");
    update_title(window, &gc.ctrl());

    let window_key_event = EventControllerKey::new();
    {
        let gc = gc.clone();
        window_key_event.connect_key_released(move |_, key, _, _| match key {
            Key::Control_L => {
                let view = &gc.ctrl().render;
                if let Some((x, y)) = mouse_pointer(view.as_ref()) {
                    update_mouse_pointer(&view, x, y, MODIFIER_NONE);
                }
            }
            _ => {}
        });
    }
    {
        let gc = gc.clone();
        window_key_event.connect_key_pressed(move |_, key, _, modifier| {
            let (key, modifier) = ignore_cap(key, modifier);
            match (key, modifier) {
                (Key::Control_L, MODIFIER_NONE) => {
                    let view = &gc.ctrl().render;
                    if let Some((x, y)) = mouse_pointer(view.as_ref()) {
                        update_mouse_pointer(&view, x, y, ModifierType::CONTROL_MASK);
                    }
                    Propagation::Proceed
                }
                (Key::c, MODIFIER_NONE) => {
                    gc.chapter_list.block_reactive(true);
                    if switch_stack(SIDEBAR_CHAPTER_LIST_NAME, &gc, true) {
                        gc.chapter_list.scroll_to_current();
                    }
                    gc.chapter_list.block_reactive(false);
                    Propagation::Stop
                }
                (Key::d, MODIFIER_NONE) => {
                    if switch_stack(SIDEBAR_DICT_NAME, &gc, true) {
                        lookup_selection(&gc);
                    }
                    Propagation::Stop
                }
                (Key::F, MODIFIER_CTRL_SHIFT) => {
                    if switch_stack(SIDEBAR_FIND_NAME, &gc, false) {
                        if find_entry.is_sensitive() {
                            find_entry.select_region(0, -1);
                            find_entry.grab_focus();
                        }
                    }
                    Propagation::Stop
                }
                (Key::slash, MODIFIER_NONE) | (Key::f, ModifierType::CONTROL_MASK) => {
                    search_box.grab_focus();
                    if let Some(pattern) = gc.ctrl().selected() {
                        search_box.set_text(pattern)
                    }
                    search_box.select_region(0, -1);
                    Propagation::Stop
                }
                (Key::g, ModifierType::CONTROL_MASK) => {
                    gc.goto();
                    Propagation::Stop
                }
                (Key::Escape, MODIFIER_NONE) => {
                    if gc.paned.position() != 0 {
                        gc.toggle_sidebar();
                        Propagation::Stop
                    } else {
                        Propagation::Proceed
                    }
                }
                (Key::x, ModifierType::CONTROL_MASK) => {
                    switch_render(&gc);
                    Propagation::Stop
                }
                (Key::r, ModifierType::CONTROL_MASK) => {
                    gc.reload_book();
                    Propagation::Stop
                }
                (Key::o, ModifierType::CONTROL_MASK) => {
                    gc.open_dialog();
                    Propagation::Stop
                }
                (Key::O, MODIFIER_CTRL_SHIFT) => {
                    gc.open_folder_dialog();
                    Propagation::Stop
                }
                (Key::h, MODIFIER_NONE) => {
                    gc.show_history();
                    Propagation::Stop
                }
                (Key::t, MODIFIER_NONE) => {
                    gc.switch_theme();
                    Propagation::Stop
                }
                (Key::T, ModifierType::SHIFT_MASK) => {
                    gc.custom_color_action.activate(None);
                    Propagation::Stop
                }
                (Key::F, ModifierType::SHIFT_MASK) => {
                    gc.custom_font_action.activate(None);
                    Propagation::Stop
                }
                (Key::S, ModifierType::SHIFT_MASK) => {
                    gc.custom_style_dialog();
                    Propagation::Stop
                }
                (Key::s, ModifierType::CONTROL_MASK) => {
                    gc.show_settings();
                    Propagation::Stop
                }
                (Key::w, ModifierType::CONTROL_MASK) => {
                    gc.window.close();
                    Propagation::Stop
                }
                (Key::i, MODIFIER_NONE) => {
                    if let Err(err) = gc.book_info() {
                        gc.error(&err.to_string());
                    }
                    Propagation::Stop
                }
                (Key::F11, MODIFIER_NONE) => {
                    let win = &gc.window;
                    win.set_fullscreened(!win.is_fullscreen());
                    Propagation::Stop
                }
                _ => {
                    // println!("window pressed, key: {key}, modifier: {:#?}", modifier);
                    Propagation::Proceed
                }
            }
        });
    }
    window.add_controller(window_key_event);

    {
        let gc = gc.clone();
        window.connect_close_request(move |_| {
            let mut controller = gc.ctrl_mut();
            if controller.reading.filename != README_TEXT_FILENAME {
                let configuration = gc.cfg_mut();
                if let Err(e) = configuration.save_reading(&mut controller.reading) {
                    eprintln!("Failed save reading info: {}", e.to_string());
                }
            }
            let mut configuration = gc.cfg_mut();
            configuration.gui.dict_font_size = gc.dm.borrow().font_size();
            if let Err(e) = configuration.save() {
                eprintln!("Failed save configuration: {}", e.to_string());
            }
            Propagation::Proceed
        });
    }

    window.present();
}

fn switch_render(gc: &GuiContext) {
    let mut configuration = gc.cfg_mut();
    let render_han = !configuration.render_han;
    configuration.render_han = render_han;
    let mut controller = gc.ctrl_mut();
    let mut render_context = gc.ctx_mut();
    controller
        .render
        .reload_render(render_han, &mut render_context);
    controller.redraw(&mut render_context);
}

#[inline]
fn setup_toolbar(
    gc: &GuiContext,
    view: &GuiView,
    lookup_entry: &SearchEntry,
    find_entry: &SearchEntry,
    dark_theme: bool,
    custom_color: Option<bool>,
    custom_font: Option<bool>,
    custom_style: Option<Option<String>>,
) -> (gtk4::Box, SearchEntry) {
    let i18n = &gc.i18n;

    let toolbar = gtk4::Box::builder().css_classes(vec!["toolbar"]).build();

    let sidebar_button = &gc.sidebar_btn;
    {
        let gc = gc.clone();
        sidebar_button.connect_clicked(move |_| {
            gc.toggle_sidebar();
        });
        toolbar.append(sidebar_button);
    }

    {
        let gc = gc.clone();
        lookup_entry.connect_stop_search(move |_| {
            gc.toggle_sidebar();
        });
    }

    {
        let gc = gc.clone();
        find_entry.connect_stop_search(move |_| {
            gc.toggle_sidebar();
        });
    }

    // add file drop support
    {
        let drop_target = DropTarget::new(File::static_type(), DragAction::COPY);
        let gc = gc.clone();
        drop_target.connect_drop(move |_, value, _, _| {
            if let Ok(file) = value.get::<File>() {
                if let Some(path) = file.path() {
                    gc.open_file(&path);
                    return true;
                }
            }
            false
        });
        view.add_controller(drop_target);
    }

    setup_main_menu(
        gc,
        view,
        dark_theme,
        custom_color,
        custom_font,
        custom_style,
    );
    toolbar.append(&gc.menu_btn);

    let search_box = SearchEntry::builder()
        .placeholder_text(i18n.msg("search-hint"))
        .activates_default(true)
        .enable_undo(true)
        .build();
    toolbar.append(&search_box);

    (toolbar, search_box)
}

fn setup_main_menu(
    gc: &GuiContext,
    view: &GuiView,
    dark_theme: bool,
    custom_color: Option<bool>,
    custom_font: Option<bool>,
    custom_style: Option<Option<String>>,
) {
    #[inline]
    fn create_action<F>(
        menu: &Menu,
        action_group: &SimpleActionGroup,
        i18n: &Rc<I18n>,
        key: &str,
        callback: F,
    ) where
        F: Fn(&SimpleAction, Option<&Variant>) + 'static,
    {
        let action = SimpleAction::new(key, None);
        append_action(menu, action_group, i18n, key, &action, callback)
    }
    fn append_action<F>(
        menu: &Menu,
        action_group: &SimpleActionGroup,
        i18n: &Rc<I18n>,
        key: &str,
        action: &SimpleAction,
        callback: F,
    ) where
        F: Fn(&SimpleAction, Option<&Variant>) + 'static,
    {
        action.connect_activate(callback);
        let title = i18n.msg(key);
        let action_name = format!("main.{}", key);
        let menu_item = MenuItem::new(Some(&title), Some(&action_name));
        menu.append_item(&menu_item);
        action_group.add_action(action);
    }

    fn append_toggle_action<F>(
        menu: &Menu,
        action_group: &SimpleActionGroup,
        i18n: &Rc<I18n>,
        key: &str,
        action: &SimpleAction,
        toggle: Option<bool>,
        callback: F,
    ) where
        F: Fn(&SimpleAction, Option<&Variant>) + 'static,
    {
        if let Some(active) = toggle {
            action.set_state(&active.to_variant());
        } else {
            action.set_state(&false.to_variant());
            action.set_enabled(false);
        }
        action.connect_activate(callback);
        let title = i18n.msg(key);
        let action_name = format!("main.{}", key);
        let menu_item = MenuItem::new(Some(&title), Some(&action_name));
        menu.append_item(&menu_item);
        action_group.add_action(action);
    }

    let button = &gc.menu_btn;

    let action_group = SimpleActionGroup::new();
    let menu = Menu::new();
    let i18n = &gc.i18n;
    let section = Menu::new();
    menu.append_section(None, &section);

    button.insert_action_group("main", Some(&action_group));

    {
        let gc = gc.clone();
        create_action(&section, &action_group, i18n, OPEN_FILE_KEY, move |_, _| {
            gc.open_dialog();
        });
    }

    {
        let gc = gc.clone();
        create_action(
            &section,
            &action_group,
            i18n,
            OPEN_FOLDER_KEY,
            move |_, _| {
                gc.open_folder_dialog();
            },
        );
    }

    gc.history_list.setup(button, &gc);
    {
        let gc = gc.clone();
        create_action(&section, &action_group, i18n, HISTORY_KEY, move |_, _| {
            gc.show_history();
        });
    }

    {
        let gc = gc.clone();
        create_action(&section, &action_group, i18n, RELOAD_KEY, move |_, _| {
            gc.reload_book();
        });
    }

    {
        let gc = gc.clone();
        create_action(&section, &action_group, i18n, BOOK_INFO_KEY, move |_, _| {
            if let Err(err) = gc.book_info() {
                gc.error(
                    &gc.i18n
                        .args_msg("failed-load-reading", vec![("error", err.to_string())]),
                );
            }
        });
    }

    {
        let gc = gc.clone();
        create_action(&section, &action_group, i18n, SETTINGS_KEY, move |_, _| {
            gc.show_settings()
        });
    }

    {
        let action = &gc.theme_action;
        let gc = gc.clone();
        append_toggle_action(
            &section,
            &action_group,
            i18n,
            THEME_KEY,
            action,
            Some(dark_theme),
            move |_, _| {
                gc.switch_theme();
            },
        );
    }

    let section = Menu::new();
    menu.append_section(None, &section);

    {
        let action = &gc.custom_color_action;
        let gc = gc.clone();
        append_toggle_action(
            &section,
            &action_group,
            i18n,
            CUSTOM_COLOR_KEY,
            action,
            custom_color,
            move |_, _| {
                gc.toggle_custom_color();
            },
        );
    }

    {
        let action = &gc.custom_font_action;
        let gc = gc.clone();
        append_toggle_action(
            &section,
            &action_group,
            i18n,
            CUSTOM_FONT_KEY,
            action,
            custom_font,
            move |_, _| {
                gc.toggle_custom_font();
            },
        );
    }

    {
        let action = &gc.custom_style_action;
        if custom_style.is_none() {
            action.set_enabled(false);
        }
        let gc = gc.clone();
        append_action(
            &section,
            &action_group,
            i18n,
            CUSTOM_STYLE_KEY,
            action,
            move |_, _| {
                gc.custom_style_dialog();
            },
        );
    }

    let pm = PopoverMenu::builder()
        .has_arrow(false)
        .position(PositionType::Bottom)
        .menu_model(&MenuModel::from(menu))
        .build();
    pm.set_parent(button);
    {
        let view = view.clone();
        pm.connect_visible_notify(move |_| {
            view.grab_focus();
        });
    }

    button.connect_clicked(move |_| pm.popup());
}

#[inline]
fn load_button_image(name: &str, icons: &IconMap, inline: bool) -> Image {
    let texture = icons.get(name).unwrap();
    let image = Image::from_paintable(Some(texture));
    if inline {
        image.set_width_request(INLINE_ICON_SIZE);
        image.set_height_request(INLINE_ICON_SIZE);
    } else {
        image.set_width_request(ICON_SIZE);
        image.set_height_request(ICON_SIZE);
    }
    image
}

#[inline]
fn create_action(name: &str) -> SimpleAction {
    SimpleAction::new(name, None)
}

#[inline]
fn create_toggle_action(name: &str) -> SimpleAction {
    SimpleAction::new_stateful(name, None, &false.to_variant())
}

fn create_toggle_button(
    active: bool,
    name: &str,
    i18n_key: &str,
    icons: &IconMap,
    i18n: &I18n,
) -> ToggleButton {
    let image = load_button_image(name, icons, false);
    let tooltip = i18n.msg(i18n_key);
    ToggleButton::builder()
        .child(&image)
        .focus_on_click(false)
        .focusable(false)
        .tooltip_text(tooltip)
        .active(active)
        .build()
}

#[inline]
fn create_button(name: &str, tooltip: Option<&str>, icons: &IconMap, inline: bool) -> Button {
    let image = load_button_image(name, icons, inline);
    let button = Button::builder()
        .child(&image)
        .focus_on_click(false)
        .focusable(false)
        .build();
    button.set_tooltip_text(tooltip);

    if inline {
        button.add_css_class("inline");
        button.set_valign(Align::Center);
    }
    button
}

#[inline(always)]
fn update_title(window: &ApplicationWindow, controller: &GuiController) {
    let name = controller.reading_book_name();
    let title = format!("{} - {}", package_name!(), name);
    window.set_title(Some(&title));
}

#[inline]
#[cfg(unix)]
fn setup_env() -> Result<bool> {
    use dirs::home_dir;
    use std::fs;

    // any better way to know if a usable backend for gtk4 available?
    if !env::var("WAYLAND_DISPLAY")
        .map_or_else(|_| env::var("DISPLAY").map_or(false, |_| true), |_| true)
    {
        return Ok(false);
    }

    let home_dir = home_dir().expect("No home folder");
    let icon_path = home_dir.join(".local/share/icons/hicolor/256x256/apps");
    if !icon_path.exists() {
        fs::create_dir_all(&icon_path)?;
    }
    {
        let icon_file = icon_path.join("tbr-icon.png");
        if !icon_file.exists() {
            fs::write(&icon_file, include_bytes!("../assets/gui/tbr-icon.png"))?;
        }
    }
    Ok(true)
}

struct GuiContextInner {
    current: Option<String>,
    cfg: Rc<RefCell<Configuration>>,
    ctrl: Rc<RefCell<GuiController>>,
    ctx: Rc<RefCell<RenderContext>>,
    dm: Rc<RefCell<DictionaryManager>>,
    opener: Rc<RefCell<Opener>>,
    window: ApplicationWindow,
    history_list: HistoryList,
    status_bar: Label,
    paned: Paned,
    sidebar_stack: Stack,
    sidebar_btn: ToggleButton,
    theme_action: SimpleAction,
    custom_color_action: SimpleAction,
    custom_font_action: SimpleAction,
    custom_style_action: SimpleAction,
    menu_btn: Button,
    chapter_list: ChapterList,
    find_list: FindList,
    annotations: annotations::AnnotationPanel,
    library_search: library_search::LibrarySearchPanel,
    indexing_sources: Rc<RefCell<HashSet<(BookId, String)>>>,
    indexed_sources: Rc<RefCell<HashSet<(BookId, String)>>>,
    annotation_store: Option<Rc<RefCell<AnnotationStore>>>,
    annotation_selection: RefCell<Option<AnnotationSelection>>,
    icons: Rc<IconMap>,
    i18n: Rc<I18n>,
    fonts: Rc<Option<UserFonts>>,
    css_provider: CssProvider,
    file_dialog: FileDialog,
    settings: Settings,
    db: Rc<RefCell<DictionaryBook>>,
}

enum ChapterListSyncMode {
    NoReload,
    Reload,
    ReloadIfNeeded(usize),
}

#[derive(Clone)]
struct GuiContext {
    inner: Rc<GuiContextInner>,
}

impl Deref for GuiContext {
    type Target = GuiContextInner;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl GuiContext {
    fn new(
        app: &Application,
        settings: Settings,
        current: Option<String>,
        cfg: &Rc<RefCell<Configuration>>,
        ctrl: &Rc<RefCell<GuiController>>,
        ctx: &Rc<RefCell<RenderContext>>,
        db: Rc<RefCell<DictionaryBook>>,
        dm: Rc<RefCell<DictionaryManager>>,
        icons: Rc<IconMap>,
        i18n: Rc<I18n>,
        fonts: Rc<Option<UserFonts>>,
        css_provider: CssProvider,
    ) -> (
        Self,
        gtk4::Box,
        gtk4::Box,
        gtk4::Box,
        gtk4::Box,
        SearchEntry,
    ) {
        let window = ApplicationWindow::builder()
            .application(app)
            .default_width(800)
            .default_height(600)
            .maximized(true)
            .title(package_name!())
            .build();

        let (chapter_list, chapter_list_view) = ChapterList::create(&icons, &i18n, &ctrl);
        let (find_list, find_list_view, find_entry) = FindList::create(&current, &i18n, &icons);
        let annotations = annotations::AnnotationPanel::new();
        let annotations_view = annotations.root().clone();
        let library_search = library_search::LibrarySearchPanel::new();
        let library_search_view = library_search.root().clone();
        let annotation_store = annotation_store()
            .ok()
            .map(|store| Rc::new(RefCell::new(store)));

        let controller = ctrl.borrow();
        let status_msg = controller.status().to_string();
        let status_bar = Label::builder()
            .label(&status_msg)
            .max_width_chars(50)
            .ellipsize(EllipsizeMode::Start)
            .tooltip_text(&status_msg)
            .halign(Align::End)
            .hexpand(true)
            .build();

        find_list.set_inner_book(controller.reading.inner_book);

        let paned = Paned::new(Orientation::Horizontal);
        let sidebar_stack = Stack::builder().vexpand(true).build();
        let sidebar_btn = create_toggle_button(false, "sidebar.svg", SIDEBAR_KEY, &icons, &i18n);
        let theme_action = create_toggle_action(THEME_KEY);
        let custom_color_action = create_toggle_action(CUSTOM_COLOR_KEY);
        let custom_font_action = create_toggle_action(CUSTOM_FONT_KEY);
        let custom_style_action = create_action(CUSTOM_STYLE_KEY);

        let file_dialog = FileDialog::new();
        file_dialog.set_title(&i18n.msg("file-open-title"));
        file_dialog.set_modal(true);
        let filter = FileFilter::new();
        for ext in controller.container_manager.book_loader.extension() {
            filter.add_suffix(&ext[1..]);
        }
        file_dialog.set_default_filter(Some(&filter));

        let history_list = HistoryList::new(controller.render.as_ref(), &cfg);
        let menu_btn = create_button("menu.svg", Some(&i18n.msg("menu")), &icons, false);

        let inner = GuiContextInner {
            current,
            cfg: cfg.clone(),
            ctrl: ctrl.clone(),
            ctx: ctx.clone(),
            dm,
            opener: Rc::new(RefCell::new(Default::default())),
            window,
            history_list,
            status_bar,
            paned,
            sidebar_stack,
            sidebar_btn,
            theme_action,
            custom_color_action,
            custom_font_action,
            custom_style_action,
            menu_btn,
            chapter_list,
            find_list,
            annotations,
            library_search,
            indexing_sources: Rc::new(RefCell::new(HashSet::new())),
            indexed_sources: Rc::new(RefCell::new(HashSet::new())),
            annotation_store,
            annotation_selection: RefCell::new(None),
            icons,
            i18n,
            fonts,
            css_provider,
            file_dialog,
            settings,
            db,
        };
        let gc = GuiContext {
            inner: Rc::new(inner),
        };
        {
            let gc = gc.clone();
            let callback_gc = gc.clone();
            gc.annotations
                .connect_export(move || callback_gc.export_annotations());
        }
        {
            let gc = gc.clone();
            let callback_gc = gc.clone();
            gc.annotations
                .connect_jump(move |annotation| callback_gc.goto_annotation(annotation));
        }
        {
            let gc = gc.clone();
            let callback_gc = gc.clone();
            gc.annotations.connect_action(move |annotation, action| {
                callback_gc.change_annotation(annotation, action)
            });
        }
        {
            let gc = gc.clone();
            let callback_gc = gc.clone();
            gc.library_search
                .connect_search(move |query| callback_gc.search_library(query));
        }
        {
            let gc = gc.clone();
            let callback_gc = gc.clone();
            gc.library_search
                .connect_jump(move |hit| callback_gc.goto_library_hit(hit));
        }
        (
            gc,
            chapter_list_view,
            find_list_view,
            annotations_view,
            library_search_view,
            find_entry,
        )
    }

    #[inline]
    fn cfg(&self) -> Ref<'_, Configuration> {
        self.cfg.borrow()
    }

    #[inline]
    fn cfg_mut(&self) -> RefMut<'_, Configuration> {
        self.cfg.borrow_mut()
    }

    #[inline]
    fn ctrl(&self) -> Ref<'_, GuiController> {
        self.ctrl.borrow()
    }

    #[inline]
    fn ctrl_mut(&self) -> RefMut<'_, GuiController> {
        self.ctrl.borrow_mut()
    }

    #[inline]
    fn ctx(&self) -> Ref<'_, RenderContext> {
        self.ctx.borrow()
    }

    #[inline]
    fn ctx_mut(&self) -> RefMut<'_, RenderContext> {
        self.ctx.borrow_mut()
    }

    #[inline]
    fn opener(&self) -> RefMut<'_, Opener> {
        self.opener.borrow_mut()
    }

    #[inline]
    fn dm(&self) -> Ref<'_, DictionaryManager> {
        self.dm.borrow()
    }

    #[inline]
    fn dm_mut(&self) -> RefMut<'_, DictionaryManager> {
        self.dm.borrow_mut()
    }

    #[inline]
    fn show_history(&self) {
        match self.cfg().history(self.current.as_ref(), None) {
            Ok(infos) => self.history_list.popup(infos),
            Err(err) => self.error(&err.to_string()),
        }
    }

    #[inline]
    fn filter_history(&self, filter_pattern: Option<&String>) -> Option<Vec<ReadingInfo>> {
        match self.cfg().history(self.current.as_ref(), filter_pattern) {
            Ok(infos) => Some(infos),
            Err(err) => {
                self.error(&err.to_string());
                None
            }
        }
    }

    fn open_dialog(&self) {
        let gc = self.clone();
        self.file_dialog
            .open(Some(&self.window), None::<&Cancellable>, move |result| {
                if let Ok(file) = result {
                    if let Some(path) = file.path() {
                        gc.open_file(&path);
                    }
                }
            });
    }

    fn open_file(&self, path: &PathBuf) {
        if let Ok(absolute_path) = path.canonicalize() {
            if let Some(filepath) = absolute_path.to_str() {
                if let Some(app) = self.window.application() {
                    app_open(&app, filepath);
                }
            }
        }
    }

    fn open_folder_dialog(&self) {
        let dialog = FileDialog::new();
        dialog.set_title(&self.i18n.msg("folder-open-title"));
        dialog.set_modal(true);

        let gc = self.clone();
        dialog.select_folder(Some(&self.window), None::<&Cancellable>, move |result| {
            if let Ok(file) = result {
                if let Some(path) = file.path() {
                    if path.is_dir() {
                        gc.open_file(&path);
                    }
                }
            }
        });
    }

    fn reload_book(&self) {
        let mut controller = self.ctrl_mut();
        let loading = BookLoadingInfo::Reload(controller.reading.clone());
        match controller.switch_container(loading, &mut self.ctx_mut()) {
            Ok(msg) => {
                drop(controller);
                self.chapter_list
                    .sync_chapter_list(ChapterListSyncMode::Reload);
                update_status(false, &msg, &self.status_bar);
                if let Err(err) = self.refresh_annotations() {
                    self.error(&err.to_string());
                }
            }
            Err(err) => self.error(&err.to_string()),
        }
    }

    fn book_info(&self) -> Result<()> {
        #[inline]
        fn label(title: &str, text: &mut String) -> Label {
            text.push('\n');
            text.push_str(title);
            Label::builder().halign(Align::Start).label(title).build()
        }

        let mut text = String::new();
        let controller = self.ctrl_mut();
        let reading = &controller.reading;
        let path = PathBuf::from_str(&reading.filename)?;
        let meta = path.metadata()?;
        let container = gtk4::Box::new(Orientation::Vertical, 10);
        container.append(&label(&reading.filename, &mut text));
        container.append(&label(&format_size(meta.len()), &mut text));
        container.append(&Separator::new(Orientation::Horizontal));
        if let Some(book_names) = controller.container.inner_book_names() {
            if let Some(name) = book_names.get(reading.inner_book) {
                container.append(&label(&name.name(), &mut text));
            }
        }
        let status = controller.status();
        if let Some(title) = status.title {
            container.append(&label(title, &mut text));
        }
        container.append(&label(&status.position(), &mut text));
        let popover = Popover::builder().child(&container).build();
        popover.set_parent(&self.menu_btn);

        let key_event = EventControllerKey::new();
        key_event.connect_key_pressed(move |ev, key, _, modifier| {
            let (key, modifier) = ignore_cap(key, modifier);
            match (key, modifier) {
                (Key::i, MODIFIER_NONE) | (Key::q, MODIFIER_NONE) => {
                    if let Some(widget) = ev.widget() {
                        widget.set_visible(false);
                    }
                    Propagation::Stop
                }
                (Key::c, ModifierType::CONTROL_MASK) => {
                    copy_to_clipboard(&text);
                    Propagation::Stop
                }
                _ => {
                    // println!("view, key: {key}, modifier: {modifier}");
                    Propagation::Proceed
                }
            }
        });
        popover.add_controller(key_event);
        popover.popup();
        Ok(())
    }

    fn toggle_sidebar(&self) {
        let paned = &self.paned;
        let (on, position) = if paned.position() == 0 {
            (true, self.cfg().gui.sidebar_size as i32)
        } else {
            self.ctrl().render.grab_focus();
            (false, 0)
        };
        self.sidebar_btn.set_active(on);
        paned.set_position(position);
    }

    fn switch_theme(&self) {
        let mut configuration = self.cfg_mut();
        let dark_theme = !configuration.dark_theme;
        self.theme_action.set_state(&dark_theme.to_variant());
        configuration.dark_theme = dark_theme;
        let mut render_context = self.ctx_mut();
        render_context.colors = configuration.curr_theme().clone();
        let mut controller = self.ctrl_mut();
        controller.redraw(&mut render_context);
        view::update_css(&self.css_provider, &render_context.colors);
    }

    fn toggle_custom_color(&self) {
        let mut controller = self.ctrl_mut();
        let custom_color = !controller.reading.custom_color;
        self.custom_color_action
            .set_state(&custom_color.to_variant());
        controller.reading.custom_color = custom_color;
        let mut render_context = self.ctx_mut();
        render_context.custom_color = custom_color;
        controller.redraw(&mut render_context);
    }

    fn toggle_custom_font(&self) {
        let mut controller = self.ctrl_mut();
        let custom_font = !controller.reading.custom_font;
        self.custom_font_action.set_state(&custom_font.to_variant());
        controller.reading.custom_font = custom_font;
        let mut render_context = self.ctx_mut();
        controller.render.set_custom_font(
            custom_font,
            controller.book.custom_fonts(),
            &mut render_context,
        );
        controller.redraw(&mut render_context);
    }

    fn custom_style_dialog(&self) {
        let controller = self.ctrl();
        let reading = &controller.reading;
        let gc = self.clone();
        dialogs::custom_styles(
            &reading.custom_style,
            self,
            &self.window,
            move |new_style| {
                let mut controller = gc.ctrl_mut();
                let custom_style = if let Some(custom_style) = &controller.reading.custom_style {
                    if new_style == *custom_style {
                        return;
                    } else if new_style.is_empty() {
                        None
                    } else {
                        Some(new_style)
                    }
                } else if new_style.is_empty() {
                    return;
                } else {
                    Some(new_style)
                };
                controller.reading.custom_style = custom_style;
                drop(controller);
                gc.reload_book();
            },
        );
    }

    fn show_annotation_composer(&self, view: &GuiView) {
        if self.annotation_selection.borrow().is_none() {
            self.error("请先选中一段文字。");
            return;
        }
        let gc = self.clone();
        annotations::show_annotation_composer(view, move |color, note, tags| {
            gc.create_annotation(color, note, tags);
        });
    }

    fn create_annotation(&self, color: AnnotationColor, note: Option<String>, tags: Vec<String>) {
        let Some(selection) = self.annotation_selection.borrow_mut().take() else {
            self.error("选区已失效，请重新选择文字。");
            return;
        };
        let (line, start_char, end_line, end_char) = if selection.from_line < selection.to_line
            || (selection.from_line == selection.to_line
                && selection.from_offset <= selection.to_offset)
        {
            (
                selection.from_line,
                selection.from_offset,
                selection.to_line,
                selection.to_offset.saturating_add(1),
            )
        } else {
            (
                selection.to_line,
                selection.to_offset,
                selection.from_line,
                selection.from_offset.saturating_add(1),
            )
        };
        let result = (|| -> Result<()> {
            let anchor = {
                let controller = self.ctrl();
                Anchor::new(
                    controller.reading.inner_book,
                    controller.reading.chapter,
                    line,
                    start_char,
                    end_line,
                    end_char,
                    controller.book.lines(),
                )
                .ok_or_else(|| anyhow!("无法从当前选区创建批注。"))?
            };
            let (book_id, metadata, path, _, _) = self
                .active_annotation_book()?
                .ok_or_else(|| anyhow!("请先打开一本本地书籍。"))?;
            let store = self.annotation_store()?;
            let mut store = store.borrow_mut();
            store.upsert_book(&book_id, &metadata.title, metadata.author.as_deref(), &path)?;
            store.create(&book_id, NewAnnotation::new(anchor, color, note, tags))?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.ctrl_mut().clear_highlight(&mut self.ctx_mut());
                match self.refresh_annotations() {
                    Err(err) => {
                        self.error(&err.to_string());
                    }
                    _ => {
                        self.message("已保存批注。");
                    }
                }
            }
            Err(err) => self.error(&err.to_string()),
        }
    }

    fn annotation_store(&self) -> Result<Rc<RefCell<AnnotationStore>>> {
        self.annotation_store
            .clone()
            .ok_or_else(|| anyhow!("批注数据库不可用。"))
    }

    fn active_annotation_book(
        &self,
    ) -> Result<Option<(BookId, BookMetadata, String, usize, usize)>> {
        let (path, metadata, inner_book, chapter) = {
            let controller = self.ctrl();
            (
                PathBuf::from(&controller.reading.filename),
                BookMetadata::new(controller.reading_book_name(), Option::<String>::None),
                controller.reading.inner_book,
                controller.reading.chapter,
            )
        };
        if !path.is_file() {
            return Ok(None);
        }
        let book_id = BookId::from_file(&path, &metadata.title, metadata.author.as_deref())?;
        Ok(Some((
            book_id,
            metadata,
            path.to_string_lossy().into_owned(),
            inner_book,
            chapter,
        )))
    }

    fn refresh_annotations(&self) -> Result<()> {
        let Some((book_id, metadata, path, inner_book, chapter)) = self.active_annotation_book()?
        else {
            self.annotations.set_annotations(vec![]);
            self.ctx_mut().persistent_highlights.clear();
            return Ok(());
        };
        let store = self.annotation_store()?;
        let annotations = {
            let mut store = store.borrow_mut();
            store.upsert_book(&book_id, &metadata.title, metadata.author.as_deref(), &path)?;
            store.list_for_book(&book_id)?
        };
        self.sync_library_chapter(
            &book_id,
            &metadata.title,
            &path,
            inner_book,
            chapter,
            &annotations,
        )?;
        self.schedule_library_index(
            book_id.clone(),
            metadata.clone(),
            path.clone(),
            annotations.clone(),
        );
        let highlights = {
            let controller = self.ctrl();
            annotations
                .iter()
                .filter(|annotation| {
                    annotation.anchor.inner_book == inner_book
                        && annotation.anchor.chapter == chapter
                })
                .flat_map(|annotation| {
                    annotation
                        .anchor
                        .ranges(controller.book.lines())
                        .unwrap_or_default()
                        .into_iter()
                        .map(|anchor| {
                            PersistentHighlight::new(
                                anchor.line,
                                anchor.range,
                                annotation_color(annotation.color),
                            )
                        })
                })
                .collect()
        };
        self.annotations.set_annotations(annotations);
        let mut context = self.ctx_mut();
        context.persistent_highlights = highlights;
        self.ctrl_mut().redraw(&mut context);
        Ok(())
    }

    fn sync_library_chapter(
        &self,
        book_id: &BookId,
        book_title: &str,
        source_path: &str,
        inner_book: usize,
        chapter: usize,
        annotations: &[Annotation],
    ) -> Result<()> {
        let chapter_title = format!("第 {} 章", chapter + 1);
        let documents = {
            let controller = self.ctrl();
            controller
                .book
                .lines()
                .iter()
                .enumerate()
                .filter_map(|(line, text)| {
                    let body = text.to_string();
                    if body.trim().is_empty() {
                        return None;
                    }
                    let related: Vec<_> = annotations
                        .iter()
                        .filter(|annotation| {
                            annotation.anchor.inner_book == inner_book
                                && annotation.anchor.chapter == chapter
                                && annotation.anchor.line <= line
                                && line <= annotation.anchor.end_line
                        })
                        .collect();
                    let note = related
                        .iter()
                        .filter_map(|annotation| annotation.note.as_deref())
                        .collect::<Vec<_>>()
                        .join("\n");
                    let tags = related
                        .iter()
                        .flat_map(|annotation| annotation.tags.clone())
                        .collect();
                    Some(LibraryDocument {
                        book_id: book_id.clone(),
                        book_title: book_title.to_owned(),
                        source_path: source_path.to_owned(),
                        chapter: chapter_title.clone(),
                        inner_book,
                        chapter_index: chapter,
                        line,
                        body,
                        note: (!note.is_empty()).then_some(note),
                        tags,
                    })
                })
                .collect::<Vec<_>>()
        };
        let mut index = library_index()?;
        index.replace_chapter(book_id, &chapter_title, &documents)
    }

    fn schedule_library_index(
        &self,
        book_id: BookId,
        metadata: BookMetadata,
        path: String,
        annotations: Vec<Annotation>,
    ) {
        let source = (book_id.clone(), path.clone());
        if self.indexed_sources.borrow().contains(&source)
            || !self.indexing_sources.borrow_mut().insert(source.clone())
        {
            return;
        }
        let database_path = match library_database_path() {
            Ok(path) => path,
            Err(err) => {
                self.indexing_sources.borrow_mut().remove(&source);
                self.error(&err.to_string());
                return;
            }
        };
        let (sender, receiver) = mpsc::channel();
        let source_path = PathBuf::from(&path);
        let title = metadata.title;
        thread::spawn(move || {
            let result = (|| -> Result<usize> {
                let documents =
                    collect_file_documents(&source_path, &book_id, &title, &annotations)?;
                let count = documents.len();
                LibraryIndex::open(&database_path)?.replace_book(&book_id, &documents)?;
                Ok(count)
            })();
            let _ = sender.send(result.map_err(|err| err.to_string()));
        });
        let gc = self.clone();
        glib::timeout_add_local(Duration::from_millis(150), move || {
            match receiver.try_recv() {
                Ok(Ok(count)) => {
                    gc.indexing_sources.borrow_mut().remove(&source);
                    gc.indexed_sources.borrow_mut().insert(source.clone());
                    match gc.refresh_annotations() {
                        Err(err) => {
                            gc.error(&err.to_string());
                        }
                        _ => {
                            gc.message(&format!("书库已索引 {count} 个片段。"));
                        }
                    }
                    glib::ControlFlow::Break
                }
                Ok(Err(err)) => {
                    gc.indexing_sources.borrow_mut().remove(&source);
                    gc.error(&format!("书库索引失败：{err}"));
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    gc.indexing_sources.borrow_mut().remove(&source);
                    gc.error("书库索引任务意外停止。");
                    glib::ControlFlow::Break
                }
            }
        });
    }

    fn goto_annotation(&self, annotation: Annotation) {
        let resolved = {
            let controller = self.ctrl();
            annotation.anchor.reconnect(controller.book.lines())
        };
        let Some(resolved) = resolved else {
            self.error("暂时无法回到原处；原文可能已经变动。");
            return;
        };
        handle(self, |controller, context| {
            controller.goto(
                annotation.anchor.inner_book,
                annotation.anchor.chapter,
                resolved.line,
                resolved.range.start,
                Some(resolved.range),
                context,
            )
        });
        if let Err(err) = self.refresh_annotations() {
            self.error(&err.to_string());
        }
    }

    fn search_library(&self, query: &str) -> Vec<SearchHit> {
        match library_index().and_then(|index| index.search(query, 50)) {
            Ok(hits) => hits,
            Err(err) => {
                self.error(&err.to_string());
                vec![]
            }
        }
    }

    fn goto_library_hit(&self, hit: SearchHit) {
        match self.active_annotation_book() {
            Ok(Some((current_book_id, _, _, _, _))) if current_book_id == hit.book_id => {
                self.goto_library_location(&hit);
            }
            Ok(_) => match hit.source_path.as_deref().map(PathBuf::from) {
                Some(path) if path.is_file() => self.open_library_hit(&path, &hit),
                _ => self.error("找不到这本书的本地文件；请重新打开该书以更新书库。"),
            },
            Err(err) => self.error(&err.to_string()),
        }
    }

    fn goto_library_location(&self, hit: &SearchHit) {
        self.goto_library_position(hit.inner_book, hit.chapter_index, hit.line);
    }

    fn goto_library_position(&self, inner_book: usize, chapter_index: usize, line: usize) {
        handle(self, |controller, context| {
            controller.goto(inner_book, chapter_index, line, 0, None, context)
        });
        if let Err(err) = self.refresh_annotations() {
            self.error(&err.to_string());
        }
    }

    fn open_library_hit(&self, path: &PathBuf, hit: &SearchHit) {
        let Ok(path) = path.canonicalize() else {
            self.error("无法打开这本书的本地文件。");
            return;
        };
        let Some(filename) = path.to_str() else {
            self.error("书籍路径不是有效文本。");
            return;
        };
        let Some(app) = self.window.application() else {
            self.error("无法访问应用程序。");
            return;
        };
        app_open_at(
            &app,
            LibraryJump {
                filename: filename.to_owned(),
                inner_book: hit.inner_book,
                chapter_index: hit.chapter_index,
                line: hit.line,
            },
        );
    }

    fn change_annotation(&self, annotation: Annotation, action: annotations::AnnotationAction) {
        if matches!(action, annotations::AnnotationAction::Edit) {
            self.edit_annotation(annotation);
            return;
        }
        let result = (|| -> Result<()> {
            let store = self.annotation_store()?;
            let mut store = store.borrow_mut();
            match action {
                annotations::AnnotationAction::Edit => unreachable!(),
                annotations::AnnotationAction::ToggleStar => {
                    store.set_starred(annotation.id, !annotation.starred)?
                }
                annotations::AnnotationAction::TogglePending => {
                    let status = if annotation.status == AnnotationStatus::Pending {
                        AnnotationStatus::Normal
                    } else {
                        AnnotationStatus::Pending
                    };
                    store.set_status(annotation.id, status)?;
                }
                annotations::AnnotationAction::Delete => store.delete(annotation.id)?,
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                if let Err(err) = self.refresh_annotations() {
                    self.error(&err.to_string());
                }
            }
            Err(err) => self.error(&err.to_string()),
        }
    }

    fn edit_annotation(&self, annotation: Annotation) {
        let gc = self.clone();
        let initial = annotation.clone();
        annotations::show_annotation_editor(&self.window, &initial, move |color, note, tags| {
            let result = (|| -> Result<()> {
                let draft = NewAnnotation::new(annotation.anchor.clone(), color, note, tags)
                    .starred(annotation.starred)
                    .status(annotation.status);
                gc.annotation_store()?
                    .borrow_mut()
                    .update(annotation.id, draft)
            })();
            match result {
                Ok(()) => match gc.refresh_annotations() {
                    Err(err) => {
                        gc.error(&err.to_string());
                    }
                    _ => {
                        gc.message("已更新批注。");
                    }
                },
                Err(err) => gc.error(&err.to_string()),
            }
        });
    }

    fn export_annotations(&self) {
        let result = (|| -> Result<(String, String)> {
            let Some((book_id, metadata, _, _, _)) = self.active_annotation_book()? else {
                bail!("请先打开一本本地书籍。");
            };
            let annotations = self.annotation_store()?.borrow().list_for_book(&book_id)?;
            let suggested_name = format!("{}-摘录.md", metadata.title);
            Ok((suggested_name, export_book(&metadata, &annotations, false)))
        })();
        let (suggested_name, markdown) = match result {
            Ok(value) => value,
            Err(err) => {
                self.error(&err.to_string());
                return;
            }
        };
        let dialog = FileDialog::new();
        dialog.set_title("导出摘录为 Markdown");
        dialog.set_initial_name(Some(&suggested_name));
        dialog.set_modal(true);
        let gc = self.clone();
        dialog.save(Some(&self.window), None::<&Cancellable>, move |result| {
            if let Ok(file) = result {
                if let Some(path) = file.path() {
                    match std::fs::write(path, &markdown) {
                        Ok(()) => gc.message("已导出 Markdown 摘录。"),
                        Err(err) => gc.error(&err.to_string()),
                    }
                }
            }
        });
    }

    fn goto(&self) {
        let gc = self.clone();
        dialogs::goto(self, &self.window, move |line_no| {
            let mut controller = gc.ctrl_mut();
            controller.goto_line(line_no, gc.ctx_mut().deref_mut())?;
            let msg = controller.status().to_string();
            gc.message(&msg);
            Ok(())
        });
    }

    #[inline]
    fn update(&self, msg: &str, chapter_list_sync_mode: ChapterListSyncMode) {
        self.message(msg);
        self.chapter_list.sync_chapter_list(chapter_list_sync_mode);
    }

    #[inline]
    fn message(&self, msg: &str) {
        update_status(false, msg, &self.status_bar);
    }

    #[inline]
    fn error(&self, msg: &str) {
        update_status(true, msg, &self.status_bar);
    }

    #[inline]
    fn show_settings(&self) {
        self.settings.dialog(self);
    }
}

fn annotation_store() -> Result<AnnotationStore> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow!("找不到本地配置目录。"))?
        .join(package_name!());
    std::fs::create_dir_all(&config_dir)?;
    AnnotationStore::open(&config_dir.join("tbr-plus.sqlite"))
}

fn library_index() -> Result<LibraryIndex> {
    LibraryIndex::open(&library_database_path()?)
}

fn library_database_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow!("找不到本地配置目录。"))?
        .join(package_name!());
    std::fs::create_dir_all(&config_dir)?;
    Ok(config_dir.join("tbr-plus.sqlite"))
}

fn annotation_color(color: AnnotationColor) -> Color32 {
    match color {
        AnnotationColor::Yellow => Color32::LIGHT_YELLOW,
        AnnotationColor::Blue => Color32::LIGHT_BLUE,
        AnnotationColor::Green => Color32::LIGHT_GREEN,
        AnnotationColor::Red => Color32::LIGHT_RED,
    }
}

fn update_status(error: bool, msg: &str, status_bar: &Label) {
    if error {
        let msg = glib::markup_escape_text(msg);
        let markup = format!("<span foreground='red'>{msg}</span>");
        status_bar.set_markup(&markup);
    } else {
        status_bar.set_text(msg);
    };
    status_bar.set_tooltip_text(Some(msg));
}

fn show(
    app: &Application,
    current: Option<String>,
    cfg: &Rc<RefCell<Configuration>>,
    gcs: &Rc<RefCell<Vec<GuiContext>>>,
) {
    match build_ui(app, current, cfg.clone(), gcs) {
        Ok(Some(gc)) => {
            // clean temp files
            app.connect_shutdown(move |_| gc.opener().cleanup());
        }
        // previous opened
        Ok(None) => {}
        Err(err) => match app.active_window() {
            Some(window) => {
                alert("Failed start tbr", &err.to_string(), &window);
            }
            _ => {
                if let Some(window) = app.windows().get(0) {
                    alert("Failed start tbr", &err.to_string(), window);
                } else {
                    eprintln!("Failed start tbr: {}", err.to_string());
                }
            }
        },
    }
}

fn mouse_pointer(view: &impl IsA<Widget>) -> Option<(f32, f32)> {
    let pointer = view.display().default_seat()?.pointer()?;
    let root = view.root()?;
    let (x, y, _) = root.surface()?.device_position(&pointer)?;
    let point = root.compute_point(view, &Point::new(x as f32, y as f32))?;
    let x = point.x();
    let y = point.y();
    if x < 0. || y < 0. || x > view.width() as f32 || y > view.height() as f32 {
        None
    } else {
        Some((x, y))
    }
}

#[inline]
fn ignore_cap(key: Key, modifier: ModifierType) -> (Key, ModifierType) {
    if modifier & ModifierType::LOCK_MASK == MODIFIER_NONE {
        (key, modifier)
    } else {
        let modifier = modifier ^ ModifierType::LOCK_MASK;
        let key = key.to_lower();
        (key, modifier)
    }
}

pub fn start(
    current: Option<String>,
    configuration: Configuration,
) -> Result<Option<(Option<String>, Configuration)>> {
    #[cfg(unix)]
    if !setup_env()? {
        return Ok(Some((current, configuration)));
    };

    let app = Application::builder()
        .flags(ApplicationFlags::HANDLES_OPEN)
        .application_id(APP_ID)
        .build();

    let mut args = env::args().collect::<Vec<_>>();
    args.drain(1..);
    let start_without_file = if let Some(filename) = current {
        args.push(filename);
        false
    } else {
        true
    };

    let cfg = Rc::new(RefCell::new(configuration));
    let gcs = Rc::new(RefCell::new(vec![]));
    {
        let cfg = cfg.clone();
        let gcs = gcs.clone();
        app.connect_startup(move |app| {
            let css_provider = CssProvider::new();
            css_provider.load_from_string(include_str!("../assets/gui/gtk.css"));
            gtk4::style_context_add_provider_for_display(
                &Display::default().expect("Could not connect to a display."),
                &css_provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
            Window::set_default_icon_name("tbr-icon");

            #[cfg(unix)]
            {
                handle_signal(2, app.clone());
                handle_signal(15, app.clone());
            }
            if start_without_file {
                show(app, None, &cfg, &gcs);
            }
        });
    }

    {
        app.connect_open(move |app, files, _| {
            if !files.is_empty() {
                if let Some(path) = files[0].path() {
                    if let Some(path) = path.to_str() {
                        let jump = take_pending_library_jump(path);
                        let current = Some(path.to_owned());
                        show(app, current, &cfg, &gcs);
                        if let Some(jump) = jump {
                            let gui_contexts = gcs.borrow();
                            if let Ok(idx) = get_gc(gui_contexts.as_ref(), &jump.filename) {
                                gui_contexts[idx].goto_library_position(
                                    jump.inner_book,
                                    jump.chapter_index,
                                    jump.line,
                                );
                            }
                        }
                        let mut gui_contexts = gcs.borrow_mut();
                        if let Ok(idx) = get_gc(gui_contexts.as_ref(), README_TEXT_FILENAME) {
                            let gc = gui_contexts.remove(idx);
                            drop(gui_contexts);
                            gc.window.close();
                        }
                    }
                }
            }
        });
    }

    if app.run_with_args::<String>(&args) == ExitCode::FAILURE {
        bail!("Failed start tbr")
    }

    Ok(None)
}

#[cfg(unix)]
fn handle_signal(signum: i32, app: Application) {
    glib::unix_signal_add_local_once(signum, move || {
        for win in app.windows() {
            win.close();
        }
    });
}

#[inline]
fn alert(title: &str, msg: &str, parent: &impl IsA<Window>) {
    AlertDialog::builder()
        .message(title)
        .detail(msg)
        .modal(true)
        .build()
        .show(Some(parent))
}

#[inline]
fn app_open(app: &Application, filepath: &str) {
    app.open(&vec![File::for_commandline_arg(filepath)], "");
}

fn app_open_at(app: &Application, jump: LibraryJump) {
    PENDING_LIBRARY_JUMP.with(|pending| pending.replace(Some(jump.clone())));
    app_open(app, &jump.filename);
}

fn take_pending_library_jump(path: &str) -> Option<LibraryJump> {
    PENDING_LIBRARY_JUMP.with(|pending| {
        pending
            .replace(None)
            .filter(|jump| jump.filename.as_str() == path)
    })
}

#[inline]
fn get_gc(gcs: &Vec<GuiContext>, filename: &str) -> core::result::Result<usize, usize> {
    gcs.binary_search_by(|gc| gc.ctrl().reading.filename.as_str().cmp(filename))
}
