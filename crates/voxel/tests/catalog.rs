use primal_voxel::{block_def, parse_block_catalog};

const CATALOG: &str = include_str!("../../../content/blocks.tsv");

#[test]
fn bundled_catalog_preserves_stable_ids_and_palette() {
    let catalog = parse_block_catalog(CATALOG).unwrap();
    assert_eq!(catalog.len(), 7);
    assert_eq!(block_def(1).unwrap().name, "Herbe");
    assert_eq!(block_def(3).unwrap().rgb, [132, 149, 142]);
    assert_eq!(block_def(7).unwrap().name, "Cristal");
    assert_eq!(block_def(7).unwrap().rgb, [75, 207, 196]);
    assert!(block_def(0).is_none());
    assert!(block_def(8).is_none());
    assert!(std::ptr::eq(block_def(1).unwrap(), block_def(1).unwrap()));
}

#[test]
fn catalog_rejects_unknown_version_duplicate_missing_or_unknown_id() {
    for invalid in [
        CATALOG.replace("primal-blocks\t1", "primal-blocks\t2"),
        CATALOG.replace("7\tCristal", "6\tCristal"),
        CATALOG.lines().take(8).collect::<Vec<_>>().join("\n"),
        CATALOG.replace("7\tCristal", "8\tCristal"),
    ] {
        assert!(parse_block_catalog(&invalid).is_err());
    }
}

#[test]
fn catalog_rejects_invalid_color_name_and_columns_with_line_number() {
    for invalid in [
        CATALOG.replace("137\t167", "256\t167"),
        CATALOG.replace("Herbe", ""),
        CATALOG.replace("137\t167", " 137\t167"),
        CATALOG.replace("137\t167\t73", "137\t167"),
    ] {
        let error = parse_block_catalog(&invalid).unwrap_err();
        assert_eq!(error.line, 3);
        assert!(!error.to_string().is_empty());
    }
    let error = parse_block_catalog(&CATALOG.replace("Terre", "Herbe")).unwrap_err();
    assert_eq!(error.line, 4);
}
