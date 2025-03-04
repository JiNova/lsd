mod date;
mod filetype;
mod indicator;
mod name;
mod owner;
mod permissions;
mod size;
mod symlink;

#[cfg(windows)]
mod windows_utils;

pub use self::date::Date;
pub use self::filetype::FileType;
pub use self::indicator::Indicator;
pub use self::name::Name;
pub use self::owner::Owner;
pub use self::permissions::Permissions;
pub use self::size::Size;
pub use self::symlink::SymLink;
pub use crate::flags::Display;
pub use crate::icon::Icons;

use std::fs;
use std::fs::read_link;
use std::io::{Error, ErrorKind};
use std::path::PathBuf;

use globset::GlobSet;

#[derive(Clone, Debug)]
pub struct Meta {
    pub name: Name,
    pub path: PathBuf,
    pub permissions: Permissions,
    pub date: Date,
    pub owner: Owner,
    pub file_type: FileType,
    pub size: Size,
    pub symlink: SymLink,
    pub indicator: Indicator,
    pub content: Option<Vec<Meta>>,
}

impl Meta {
    pub fn recurse_into(
        &self,
        depth: usize,
        display: Display,
        ignore_globs: &GlobSet,
    ) -> Result<Option<Vec<Meta>>, std::io::Error> {
        if depth == 0 {
            return Ok(None);
        }

        if display == Display::DisplayDirectoryItself {
            return Ok(None);
        }

        match self.file_type {
            FileType::Directory { .. } => (),
            _ => return Ok(None),
        }

        let entries = match self.path.read_dir() {
            Ok(entries) => entries,
            Err(err) => {
                eprintln!("cannot access '{}': {}", self.path.display(), err);
                return Ok(None);
            }
        };

        let mut content: Vec<Meta> = Vec::new();

        if let Display::DisplayAll = display {
            let mut current_meta;
            let mut parent_meta;

            let absolute_path = fs::canonicalize(&self.path)?;
            let parent_path = match absolute_path.parent() {
                None => PathBuf::from("/"),
                Some(path) => PathBuf::from(path),
            };

            current_meta = self.clone();
            current_meta.name.name = ".".to_string();

            parent_meta = Self::from_path(&parent_path)?;
            parent_meta.name.name = "..".to_string();

            content.push(current_meta);
            content.push(parent_meta);
        }

        for entry in entries {
            let path = entry?.path();

            let name = path
                    .file_name()
                    .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "invalid file name"))?;

            if ignore_globs.is_match(&name) {
                continue;
            }

            if let Display::DisplayOnlyVisible = display {
                if name.to_string_lossy().starts_with('.')
                {
                    continue;
                }
            }

            let mut entry_meta = match Self::from_path(&path) {
                Ok(res) => res,
                Err(err) => {
                    eprintln!("cannot access '{}': {}", path.display(), err);
                    continue;
                }
            };

            match entry_meta.recurse_into(depth - 1, display, ignore_globs) {
                Ok(content) => entry_meta.content = content,
                Err(err) => {
                    eprintln!("cannot access '{}': {}", path.display(), err);
                    continue;
                }
            };

            content.push(entry_meta);
        }

        Ok(Some(content))
    }

    pub fn calculate_total_size(&mut self) {
        if let FileType::Directory { uid: _ } = self.file_type {
            if let Some(metas) = &mut self.content {
                let mut size_accumulated = self.size.get_bytes();
                for x in &mut metas.iter_mut() {
                    x.calculate_total_size();
                    size_accumulated += x.size.get_bytes();
                }
                self.size = Size::new(size_accumulated);
            } else {
                // possibility that 'depth' limited the recursion in 'recurse_into'
                self.size = Size::new(Meta::calculate_total_file_size(&self.path));
            }
        }
    }

    fn calculate_total_file_size(path: &PathBuf) -> u64 {
        let metadata = if read_link(&path).is_ok() {
            // If the file is a link, retrieve the metadata without following
            // the link.
            path.symlink_metadata()
        } else {
            path.metadata()
        };
        let metadata = match metadata {
            Ok(meta) => meta,
            Err(err) => {
                eprintln!("cannot access '{}': {}", path.display(), err);
                return 0;
            }
        };
        let file_type = metadata.file_type();
        if file_type.is_file() {
            metadata.len()
        } else if file_type.is_dir() {
            let mut size = metadata.len();

            let entries = match path.read_dir() {
                Ok(entries) => entries,
                Err(err) => {
                    eprintln!("cannot access '{}': {}", path.display(), err);
                    return size;
                }
            };
            for entry in entries {
                let path = match entry {
                    Ok(entry) => entry.path(),
                    Err(err) => {
                        eprintln!("cannot access '{}': {}", path.display(), err);
                        continue;
                    }
                };
                size += Meta::calculate_total_file_size(&path);
            }
            size
        } else {
            0
        }
    }

    pub fn from_path(path: &PathBuf) -> Result<Self, std::io::Error> {
        let metadata = if read_link(path).is_ok() {
            // If the file is a link, retrieve the metadata without following
            // the link.
            path.symlink_metadata()?
        } else {
            path.metadata()?
        };

        #[cfg(unix)]
        let owner = Owner::from(&metadata);
        #[cfg(unix)]
        let permissions = Permissions::from(&metadata);

        #[cfg(windows)]
        let (owner, permissions) = windows_utils::get_file_data(&path)?;

        let file_type = FileType::new(&metadata, &permissions);
        let name = Name::new(&path, file_type);

        Ok(Self {
            path: path.to_path_buf(),
            symlink: SymLink::from(path.as_path()),
            size: Size::from(&metadata),
            date: Date::from(&metadata),
            indicator: Indicator::from(file_type),
            owner,
            permissions,
            name,
            file_type,
            content: None,
        })
    }
}
