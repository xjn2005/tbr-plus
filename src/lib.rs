#[macro_use]
extern crate markup5ever;

use anyhow::{Result, anyhow};
use rust_embed::RustEmbed;

use crate::book::BookLoader;
use crate::common::Position;
use crate::config::load_config;
use crate::container::ContainerManager;
#[cfg(feature = "i18n")]
use crate::i18n::I18n;

#[cfg(any(feature = "gui", test))]
mod annotations;
mod book;
mod color;
mod common;
mod config;
mod container;
mod controller;
#[cfg(feature = "gui")]
mod gui;
mod html_parser;
#[cfg(feature = "i18n")]
mod i18n;
#[cfg(any(feature = "gui", test))]
mod library;
mod list;
#[cfg(feature = "open")]
mod open;
mod terminal;
mod xhtml;

const TBR_BOOK_ENV_KEY: &str = "TBR_BOOK";

#[macro_export]
macro_rules! description {
    () => {
        "Local-first ebook reader,"
    };
}

#[macro_export]
macro_rules! version {
    () => {
        env!("CARGO_PKG_VERSION")
    };
}

#[macro_export]
macro_rules! version_string {
    () => {
        concat!(description!(), " v", version!())
    };
}

#[macro_export]
macro_rules! package_name {
    () => {
        env!("CARGO_PKG_NAME")
    };
}

#[derive(RustEmbed)]
#[folder = "assets/"]
#[prefix = ""]
#[include = "*.toml"]
#[include = "*.svg"]
#[include = "*.ttc"]
#[include = "*.ftl"]
#[include = "*.png"]
struct Asset;

pub struct LaunchOptions {
    pub terminal: bool,
    pub filename: Option<String>,
}

pub fn run(options: LaunchOptions) -> Result<()> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow!("Can not find config dir."))?
        .join(package_name!());
    let cache_dir = dirs::cache_dir()
        .ok_or_else(|| anyhow!("Can not find cache dir."))?
        .join(package_name!());
    let config_file = config_dir.join("tbr.toml");
    let filename = options
        .filename
        .map_or_else(|| std::env::var(TBR_BOOK_ENV_KEY).ok(), Some);
    #[allow(unused_mut)]
    let (mut current, mut configuration) =
        load_config(filename, config_file, &config_dir, &cache_dir)?;
    #[cfg(feature = "gui")]
    if !options.terminal {
        match gui::start(current, configuration)? {
            Some((curr, config)) => {
                current = curr;
                configuration = config;
            }
            None => return Ok(()),
        }
    }
    terminal::start(current, configuration, config_dir)
}
