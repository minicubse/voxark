use std::fmt;
use std::sync::OnceLock;

use crate::MAX_BLOCK;

const BUNDLED_CATALOG: &str = include_str!("../../../content/blocks.tsv");
static BLOCKS: OnceLock<Vec<BlockDef>> = OnceLock::new();

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockDef {
    pub id: u8,
    pub name: String,
    pub rgb: [u8; 3],
}

#[derive(Debug, PartialEq, Eq)]
pub struct CatalogError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "block catalog line {}: {}",
            self.line, self.message
        )
    }
}

impl std::error::Error for CatalogError {}

/// IDs are the persisted block identity. Version 1 requires all IDs 1 through 7.
/// The table is embedded at build time and parsed once on first lookup.
pub fn block_def(id: u8) -> Option<&'static BlockDef> {
    let index = usize::from(id.checked_sub(1)?);
    BLOCKS
        .get_or_init(|| {
            parse_block_catalog(BUNDLED_CATALOG).expect("invalid bundled block catalog")
        })
        .get(index)
}

pub fn parse_block_catalog(source: &str) -> Result<Vec<BlockDef>, CatalogError> {
    let mut lines = source.lines();
    if lines.next() != Some("primal-blocks\t1") {
        return Err(error(1, "expected primal-blocks schema version 1"));
    }
    if lines.next() != Some("id\tname_fr\tr\tg\tb") {
        return Err(error(2, "expected columns id, name_fr, r, g, b"));
    }
    let mut blocks: Vec<Option<BlockDef>> = vec![None; usize::from(MAX_BLOCK)];
    for (index, row) in lines.enumerate() {
        let line = index + 3;
        let fields: Vec<_> = row.split('\t').collect();
        if fields.len() != 5 {
            return Err(error(line, "expected exactly five tab-separated fields"));
        }
        let id = number(fields[0], line)?;
        if !(1..=MAX_BLOCK).contains(&id) {
            return Err(error(line, "block ID must be in 1..=7"));
        }
        if blocks[usize::from(id - 1)].is_some() {
            return Err(error(line, "duplicate block ID"));
        }
        let name = fields[1];
        if name.trim() != name
            || name.is_empty()
            || name.chars().count() > 64
            || name.chars().any(char::is_control)
        {
            return Err(error(
                line,
                "name must contain 1..=64 characters without surrounding whitespace or controls",
            ));
        }
        if blocks.iter().flatten().any(|block| block.name == name) {
            return Err(error(line, "duplicate block name"));
        }
        let rgb = [
            number(fields[2], line)?,
            number(fields[3], line)?,
            number(fields[4], line)?,
        ];
        blocks[usize::from(id - 1)] = Some(BlockDef {
            id,
            name: name.to_owned(),
            rgb,
        });
    }
    if blocks.iter().any(Option::is_none) {
        return Err(error(
            source.lines().count() + 1,
            "missing required block ID",
        ));
    }
    Ok(blocks.into_iter().flatten().collect())
}

fn number(source: &str, line: usize) -> Result<u8, CatalogError> {
    if source.is_empty() || !source.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(error(line, "expected an unsigned integer from 0 to 255"));
    }
    source
        .parse()
        .map_err(|_| error(line, "integer exceeds 255"))
}

fn error(line: usize, message: &str) -> CatalogError {
    CatalogError {
        line,
        message: message.to_owned(),
    }
}
