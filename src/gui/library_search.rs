use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Label, ListBox, Orientation, ScrolledWindow, SearchEntry};

use crate::library::SearchHit;

type SearchCallback = Box<dyn Fn(&str) -> Vec<SearchHit>>;
type JumpCallback = Box<dyn Fn(SearchHit)>;

pub(super) struct LibrarySearchPanel {
    root: GtkBox,
    search_callback: Rc<RefCell<Option<SearchCallback>>>,
    jump_callback: Rc<RefCell<Option<JumpCallback>>>,
}

impl LibrarySearchPanel {
    pub(super) fn new() -> Self {
        let root = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(10)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        let entry = SearchEntry::builder()
            .placeholder_text("搜索整个书库、批注或标签")
            .build();
        let list = ListBox::new();
        list.add_css_class("boxed-list");
        let hits = Rc::new(RefCell::new(vec![]));
        let search_callback = Rc::new(RefCell::new(None::<SearchCallback>));
        let jump_callback = Rc::new(RefCell::new(None::<JumpCallback>));
        {
            let list = list.clone();
            let hits = hits.clone();
            let search_callback = search_callback.clone();
            entry.connect_activate(move |entry| {
                if let Some(search) = search_callback.borrow().as_ref() {
                    set_hits(&list, &hits, search(&entry.text()));
                }
            });
        }
        {
            let hits = hits.clone();
            let jump_callback = jump_callback.clone();
            list.connect_row_activated(move |_, row| {
                if let (Some(hit), Some(jump)) = (
                    hits.borrow().get(row.index() as usize),
                    jump_callback.borrow().as_ref(),
                ) {
                    jump(hit.clone());
                }
            });
        }
        root.append(&entry);
        root.append(&ScrolledWindow::builder().child(&list).vexpand(true).build());
        Self {
            root,
            search_callback,
            jump_callback,
        }
    }

    pub(super) fn root(&self) -> &GtkBox {
        &self.root
    }

    pub(super) fn connect_search(&self, callback: impl Fn(&str) -> Vec<SearchHit> + 'static) {
        self.search_callback.replace(Some(Box::new(callback)));
    }

    pub(super) fn connect_jump(&self, callback: impl Fn(SearchHit) + 'static) {
        self.jump_callback.replace(Some(Box::new(callback)));
    }
}

fn set_hits(list: &ListBox, hits: &Rc<RefCell<Vec<SearchHit>>>, results: Vec<SearchHit>) {
    while let Some(child) = { list.first_child() } {
        list.remove(&child);
    }
    if results.is_empty() {
        let label = Label::builder()
            .label("没有匹配；继续读下去，书库会逐章建立索引。")
            .wrap(true)
            .halign(Align::Start)
            .build();
        label.add_css_class("dim-label");
        list.append(&label);
    } else {
        for hit in &results {
            let card = GtkBox::builder()
                .orientation(Orientation::Vertical)
                .spacing(5)
                .margin_top(10)
                .margin_bottom(10)
                .margin_start(12)
                .margin_end(12)
                .build();
            card.add_css_class("library-search-card");
            let title = Label::builder()
                .label(format!("{} · {}", hit.book_title, hit.chapter))
                .halign(Align::Start)
                .build();
            title.add_css_class("caption");
            let body = Label::builder()
                .label(format!("… {} …", hit.body))
                .wrap(true)
                .halign(Align::Start)
                .build();
            card.append(&title);
            card.append(&body);
            if let Some(note) = &hit.note {
                let note = Label::builder()
                    .label(format!("我的批注：{note}"))
                    .wrap(true)
                    .halign(Align::Start)
                    .build();
                note.add_css_class("accent");
                card.append(&note);
            }
            list.append(&card);
        }
    }
    *hits.borrow_mut() = results;
}
