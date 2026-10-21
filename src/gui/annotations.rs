use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, Entry, Label, ListBox, ListBoxRow, Orientation, Popover,
    ScrolledWindow, TextView, WrapMode,
};

use crate::annotations::{Annotation, AnnotationColor};

#[derive(Clone, Copy)]
pub(super) enum AnnotationAction {
    Edit,
    ToggleStar,
    TogglePending,
    Delete,
}

pub(super) struct AnnotationPanel {
    root: GtkBox,
    list: ListBox,
    annotations: Rc<RefCell<Vec<Annotation>>>,
    export: Button,
    jump_callback: Rc<RefCell<Option<std::boxed::Box<dyn Fn(Annotation)>>>>,
    action_callback: Rc<RefCell<Option<std::boxed::Box<dyn Fn(Annotation, AnnotationAction)>>>>,
}

impl AnnotationPanel {
    pub(super) fn new() -> Self {
        let root = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(10)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();

        let header = GtkBox::new(Orientation::Horizontal, 8);
        let title = Label::builder()
            .label("批注")
            .halign(Align::Start)
            .hexpand(true)
            .build();
        title.add_css_class("heading");
        let export = Button::with_label("导出 Markdown");
        export.add_css_class("flat");
        header.append(&title);
        header.append(&export);
        root.append(&header);

        let list = ListBox::new();
        list.add_css_class("boxed-list");
        let annotations = Rc::new(RefCell::new(Vec::<Annotation>::new()));
        let jump_callback: Rc<RefCell<Option<std::boxed::Box<dyn Fn(Annotation)>>>> =
            Rc::new(RefCell::new(None));
        let action_callback: Rc<
            RefCell<Option<std::boxed::Box<dyn Fn(Annotation, AnnotationAction)>>>,
        > = Rc::new(RefCell::new(None));
        {
            let annotations = annotations.clone();
            let jump_callback = jump_callback.clone();
            list.connect_row_activated(move |_, row| {
                if let Some(annotation) = annotations.borrow().get(row.index() as usize)
                    && let Some(callback) = &*jump_callback.borrow()
                {
                    callback(annotation.clone());
                }
            });
        }
        let scroll = ScrolledWindow::builder()
            .child(&list)
            .vexpand(true)
            .min_content_width(260)
            .build();
        root.append(&scroll);

        Self {
            root,
            list,
            annotations,
            export,
            jump_callback,
            action_callback,
        }
    }

    pub(super) fn connect_export(&self, callback: impl Fn() + 'static) {
        self.export.connect_clicked(move |_| callback());
    }

    pub(super) fn connect_jump(&self, callback: impl Fn(Annotation) + 'static) {
        *self.jump_callback.borrow_mut() = Some(std::boxed::Box::new(callback));
    }

    pub(super) fn connect_action(&self, callback: impl Fn(Annotation, AnnotationAction) + 'static) {
        *self.action_callback.borrow_mut() = Some(std::boxed::Box::new(callback));
    }

    pub(super) fn root(&self) -> &GtkBox {
        &self.root
    }

    pub(super) fn set_annotations(&self, annotations: Vec<Annotation>) {
        while let Some(child) = { self.list.first_child() } {
            self.list.remove(&child);
        }
        if annotations.is_empty() {
            let empty = Label::builder()
                .label("选中一段文字，留下值得回来的痕迹。")
                .wrap(true)
                .halign(Align::Start)
                .margin_top(16)
                .margin_bottom(16)
                .margin_start(8)
                .margin_end(8)
                .build();
            empty.add_css_class("dim-label");
            self.list.append(&empty);
            self.annotations.borrow_mut().clear();
            return;
        }

        for annotation in &annotations {
            self.list
                .append(&annotation_row(annotation, self.action_callback.clone()));
        }
        *self.annotations.borrow_mut() = annotations;
    }
}

fn annotation_row(
    annotation: &Annotation,
    action_callback: Rc<RefCell<Option<std::boxed::Box<dyn Fn(Annotation, AnnotationAction)>>>>,
) -> ListBoxRow {
    let content = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(12)
        .margin_end(12)
        .build();
    let location = Label::builder()
        .label(format!(
            "第 {} 章 · 第 {} 行",
            annotation.anchor.chapter + 1,
            annotation.anchor.line + 1
        ))
        .halign(Align::Start)
        .build();
    location.add_css_class("caption");
    location.add_css_class("dim-label");
    let quote = Label::builder()
        .label(format!("“{}”", annotation.anchor.quote()))
        .wrap(true)
        .halign(Align::Start)
        .build();
    quote.add_css_class("annotation-quote");
    content.append(&location);
    content.append(&quote);
    if let Some(note) = &annotation.note {
        let note = Label::builder()
            .label(note)
            .wrap(true)
            .halign(Align::Start)
            .build();
        note.add_css_class("annotation-note");
        content.append(&note);
    }
    if !annotation.tags.is_empty() {
        let tags = Label::builder()
            .label(
                annotation
                    .tags
                    .iter()
                    .map(|tag| format!("#{tag}"))
                    .collect::<Vec<_>>()
                    .join("  "),
            )
            .wrap(true)
            .halign(Align::Start)
            .build();
        tags.add_css_class("accent");
        content.append(&tags);
    }
    let actions = GtkBox::new(Orientation::Horizontal, 4);
    let edit = Button::with_label("编辑");
    let star = Button::with_label(if annotation.starred {
        "★ 已收藏"
    } else {
        "☆ 收藏"
    });
    let pending = Button::with_label(
        if matches!(
            annotation.status,
            crate::annotations::AnnotationStatus::Pending
        ) {
            "待整理"
        } else {
            "标为待整理"
        },
    );
    let delete = Button::with_label("删除");
    delete.add_css_class("destructive-action");
    for (button, action) in [
        (&edit, AnnotationAction::Edit),
        (&star, AnnotationAction::ToggleStar),
        (&pending, AnnotationAction::TogglePending),
        (&delete, AnnotationAction::Delete),
    ] {
        button.add_css_class("flat");
        let annotation = annotation.clone();
        let action_callback = action_callback.clone();
        button.connect_clicked(move |_| {
            if let Some(callback) = &*action_callback.borrow() {
                callback(annotation.clone(), action);
            }
        });
    }
    actions.append(&edit);
    actions.append(&star);
    actions.append(&pending);
    actions.append(&delete);
    content.append(&actions);
    let row = ListBoxRow::builder().child(&content).build();
    row.add_css_class("annotation-card");
    row
}

pub(super) fn show_annotation_composer(
    parent: &impl IsA<gtk4::Widget>,
    on_save: impl Fn(AnnotationColor, Option<String>, Vec<String>) + 'static,
) {
    show_composer(parent, None, on_save);
}

pub(super) fn show_annotation_editor(
    parent: &impl IsA<gtk4::Widget>,
    annotation: &Annotation,
    on_save: impl Fn(AnnotationColor, Option<String>, Vec<String>) + 'static,
) {
    show_composer(parent, Some(annotation), on_save);
}

fn show_composer(
    parent: &impl IsA<gtk4::Widget>,
    annotation: Option<&Annotation>,
    on_save: impl Fn(AnnotationColor, Option<String>, Vec<String>) + 'static,
) {
    let composer = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(10)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .width_request(300)
        .build();
    let hint = Label::builder()
        .label("保存高亮，可选地写下想法和标签。")
        .halign(Align::Start)
        .wrap(true)
        .build();
    hint.add_css_class("dim-label");
    let note = TextView::builder()
        .wrap_mode(WrapMode::WordChar)
        .height_request(88)
        .build();
    let tags = Entry::builder()
        .placeholder_text("标签，以空格或逗号分隔")
        .build();
    if let Some(annotation) = annotation {
        note.buffer()
            .set_text(annotation.note.as_deref().unwrap_or_default());
        tags.set_text(&annotation.tags.join(" "));
    }
    let colors = GtkBox::new(Orientation::Horizontal, 6);
    composer.append(&hint);
    composer.append(
        &Label::builder()
            .label("我的批注")
            .halign(Align::Start)
            .build(),
    );
    composer.append(&note);
    composer.append(&tags);
    composer.append(&colors);

    let popover = Popover::builder().child(&composer).has_arrow(true).build();
    popover.set_parent(parent);
    let on_save: Rc<dyn Fn(AnnotationColor, Option<String>, Vec<String>)> = Rc::new(on_save);
    for (label, css_class, color) in [
        ("黄色", "annotation-color-yellow", AnnotationColor::Yellow),
        ("蓝色", "annotation-color-blue", AnnotationColor::Blue),
        ("绿色", "annotation-color-green", AnnotationColor::Green),
        ("红色", "annotation-color-red", AnnotationColor::Red),
    ] {
        let button = Button::with_label(label);
        button.add_css_class("flat");
        button.add_css_class(css_class);
        let note = note.clone();
        let tags = tags.clone();
        let on_save = on_save.clone();
        let popover = popover.clone();
        button.connect_clicked(move |_| {
            let buffer = note.buffer();
            let note = buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), false)
                .to_string();
            let tags = tags
                .text()
                .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == '，')
                .map(ToOwned::to_owned)
                .collect();
            on_save(color, (!note.trim().is_empty()).then_some(note), tags);
            popover.popdown();
        });
        colors.append(&button);
    }
    popover.popup();
    note.grab_focus();
}
