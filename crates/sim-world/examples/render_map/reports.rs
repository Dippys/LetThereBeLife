//! Text reports written alongside the review set: manifest, class
//! distributions, representation sizes, seed roles, and source revision.

use super::*;

pub(super) fn review_manifest(source_revision: &str, rendered: &[RenderedView]) -> String {
    let mut report = "review_format\tsource_revision\tview\tseed\tmin_x\tmin_y\tmax_x\tmax_y\tstep\tcolumns\trows\tfeatures\tsample_hash\tpath\n".to_owned();
    for output in rendered {
        let view = output.view;
        let (columns, rows) = view.dimensions();
        writeln!(
            report,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:016x}\t{}",
            REVIEW_FORMAT_VERSION,
            source_revision,
            view.name,
            view.seed,
            view.bounds.min.x,
            view.bounds.min.y,
            view.bounds.max.x,
            view.bounds.max.y,
            view.step,
            columns,
            rows,
            view.show_features,
            output.stats.sample_hash,
            output.relative_path.display(),
        )
        .expect("writing to a String cannot fail");
    }
    report
}

pub(super) fn distribution_report(rendered: &[RenderedView]) -> String {
    let mut report = String::from(
        "view\tseed\tsamples\tdeep_water\tshallow_water\tsand\tsoil\thill\trock\tsnow_ice\tocean\tlake\triver\tbeach\tdesert\tgrassland\tsavanna\tforest\twetland\ttundra\talpine\ttrees\trocks\tberry_bushes\tfeatures\tfeature_ppm\tsample_hash\n",
    );
    for output in rendered {
        let surfaces = &output.stats.surfaces;
        let biomes = &output.stats.biomes;
        let features = &output.stats.features;
        writeln!(
            report,
            concat!(
                "{}\t{}\t{}",
                "\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                "\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                "\t{}\t{}\t{}",
                "\t{}\t{}",
                "\t{:016x}"
            ),
            output.view.name,
            output.view.seed,
            output.stats.samples,
            surfaces[0],
            surfaces[1],
            surfaces[2],
            surfaces[3],
            surfaces[4],
            surfaces[5],
            surfaces[6],
            biomes[0],
            biomes[1],
            biomes[2],
            biomes[3],
            biomes[4],
            biomes[5],
            biomes[6],
            biomes[7],
            biomes[8],
            biomes[9],
            biomes[10],
            features[0],
            features[1],
            features[2],
            output.stats.feature_count(),
            output.stats.feature_parts_per_million(),
            output.stats.sample_hash,
        )
        .expect("writing to a String cannot fail");
    }
    report
}

pub(super) fn representation_report() -> String {
    let mut report = String::from("type\tsize_bytes\talign_bytes\n");
    for (name, size, align) in [
        type_layout::<SurfaceType>("SurfaceType"),
        type_layout::<BiomeType>("BiomeType"),
        type_layout::<TerrainClass>("TerrainClass"),
        type_layout::<TerrainCell>("TerrainCell"),
        type_layout::<PrevailingWind>("PrevailingWind"),
        type_layout::<ClimateSample>("ClimateSample"),
        type_layout::<FeatureKind>("FeatureKind"),
        type_layout::<Feature>("Feature"),
        type_layout::<Material>("Material"),
        type_layout::<BaseResource>("BaseResource"),
        type_layout::<GeneratedCell>("GeneratedCell"),
        type_layout::<ChunkCoord>("ChunkCoord"),
    ] {
        writeln!(report, "{name}\t{size}\t{align}").expect("writing to a String cannot fail");
    }
    report
}

fn type_layout<T>(name: &'static str) -> (&'static str, usize, usize) {
    (name, std::mem::size_of::<T>(), std::mem::align_of::<T>())
}

pub(super) fn seed_roles_report() -> String {
    let mut report = String::from("seed\trole\n");
    for case in REPRESENTATIVE_SEEDS {
        writeln!(report, "{}\t{}", case.seed, case.role).expect("writing to a String cannot fail");
    }
    report
}

pub(super) fn detect_source_revision() -> String {
    let revision = std::process::Command::new("git")
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|revision| revision.trim().to_owned())
        .filter(|revision| !revision.is_empty())
        .unwrap_or_else(|| "unavailable".to_owned());
    let dirty = std::process::Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .ok()
        .is_some_and(|output| output.status.success() && !output.stdout.is_empty());
    if dirty {
        format!("{revision}+dirty")
    } else {
        revision
    }
}

pub(super) fn sanitize_field(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\t' | '\r' | '\n' => ' ',
            other => other,
        })
        .collect()
}
