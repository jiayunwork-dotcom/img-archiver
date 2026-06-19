mod album;
mod archiver;
mod cli;
mod dedup;
mod geocoder;
mod geo_data;
mod index;
mod metadata;
mod phash;
mod report;
mod scanner;
mod template;
mod types;
mod undo;

use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashMap;
use types::*;

fn main() {
    let cli_args = cli::Cli::parse();

    match cli_args.command {
        Some(cli::Commands::Album {
            input,
            output_album,
            time_gap,
            geo_radius,
            rebuild,
        }) => {
            execute_album(&input, &output_album, &time_gap, geo_radius, rebuild);
            return;
        }
        Some(cli::Commands::Archive {
            source,
            output,
            template,
            mode,
            exclude,
            yes,
            dry_run,
            create_dirs,
            seq_digits,
            unknown_placeholder,
            config,
            stats,
            report_json,
            report_html,
            incremental,
            undo,
        }) => {
            let mut args = cli::Cli {
                command: None,
                source: Some(source),
                output: Some(output),
                template,
                mode,
                exclude,
                yes,
                dry_run,
                create_dirs,
                seq_digits,
                unknown_placeholder,
                config,
                stats,
                report_json,
                report_html,
                incremental,
                undo,
            };
            execute_archive(&mut args);
            return;
        }
        None => {
            if cli_args.source.is_some() {
                let mut args = cli_args.clone();
                execute_archive(&mut args);
                return;
            }
            eprintln!("Error: No subcommand or legacy args provided.");
            eprintln!("Use 'img-archiver --help' for usage information.");
            eprintln!("Subcommands: archive (or legacy positional args), album");
            std::process::exit(1);
        }
    }
}

fn execute_album(
    input: &std::path::Path,
    output_album: &std::path::Path,
    time_gap: &str,
    geo_radius: f64,
    rebuild: bool,
) {
    if !input.exists() {
        eprintln!("Error: Input archive directory does not exist: {}", input.display());
        std::process::exit(1);
    }

    match album::run_album(input, output_album, time_gap, geo_radius, rebuild) {
        Ok(_) => eprintln!("相册生成完成。"),
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

fn execute_archive(args: &mut cli::Cli) {
    let source = match args.source.clone() {
        Some(s) => s,
        None => {
            eprintln!("Error: Source directory is required.");
            std::process::exit(1);
        }
    };
    let output = match args.output.clone() {
        Some(o) => o,
        None => {
            eprintln!("Error: Output directory (--output) is required.");
            std::process::exit(1);
        }
    };

    if let Some(ref config_path) = args.config {
        match cli::AppConfig::load(config_path) {
            Ok(config) => config.apply_to_cli(args),
            Err(e) => {
                eprintln!("Error loading config: {}", e);
                std::process::exit(1);
            }
        }
    }

    if args.undo {
        execute_undo(&output);
        return;
    }

    let mode = match ArchiveMode::from_str(&args.mode) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    if !source.exists() {
        eprintln!("Error: Source directory does not exist: {}", source.display());
        std::process::exit(1);
    }

    if !output.exists() && !args.create_dirs {
        eprintln!("Error: Output directory does not exist: {}. Use --create-dirs to auto-create.", output.display());
        std::process::exit(1);
    }

    if !output.exists() && args.create_dirs {
        if let Err(e) = std::fs::create_dir_all(&output) {
            eprintln!("Error: Failed to create output directory: {}", e);
            std::process::exit(1);
        }
    }

    let scanner = match scanner::Scanner::new(&source, &args.exclude) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    eprintln!("Scanning directory: {}", source.display());
    let entries = match scanner.scan() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Error scanning: {}", e);
            std::process::exit(1);
        }
    };

    if entries.is_empty() {
        eprintln!("No image files found.");
        std::process::exit(0);
    }

    let summary = scanner::build_summary(&entries);
    scanner::print_summary(&summary);

    if !args.yes && !args.dry_run {
        if !scanner::confirm() {
            eprintln!("Aborted.");
            std::process::exit(0);
        }
    }

    let mut archive_index = index::ArchiveIndex::load(&output).unwrap_or_else(|e| {
        eprintln!("Warning: Could not load archive index ({}). Starting fresh.", e);
        index::ArchiveIndex::new()
    });

    let existing_sha256 = archive_index.sha256_set();

    let mut dedup = dedup::Deduplicator::new();
    dedup.load_from_index(&archive_index);

    let mut geocoder = geocoder::GeoCoder::new();

    let pb = ProgressBar::new(entries.len() as u64);
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} ({eta})",
        )
        .unwrap()
        .progress_chars("#>-"),
    );

    let archiver = archiver::Archiver::new(mode, args.dry_run, args.create_dirs);
    let mut run_stats = report::RunStats::new();
    let mut all_images: Vec<ImageInfo> = Vec::new();
    let mut suspected_dup_targets: Vec<(std::path::PathBuf, std::path::PathBuf)> = Vec::new();
    let mut suspected_dups_with_distance: Vec<(String, String, u32)> = Vec::new();
    let mut seq_counter: HashMap<String, usize> = HashMap::new();

    let mut undo_record = undo::UndoRecord::new(mode);

    let mut json_report = report::JsonReport::new();
    json_report.summary.total_images = entries.len();

    for (path, file_size, format) in &entries {
        pb.set_message(format!("Processing {}", path.display()));

        let heic_unsupported = *format == ImageFormat::Heic && !metadata::is_heic_supported();

        if heic_unsupported {
            if args.incremental {
                let sha256 = match dedup::Deduplicator::compute_sha256(path) {
                    Ok(h) => h,
                    Err(e) => {
                        run_stats.failed += 1;
                        run_stats.failures.push((path.display().to_string(), e.clone()));
                        json_report.failed.push(report::JsonImageEntry {
                            source_path: path.display().to_string(),
                            archive_path: None,
                            camera: "unknown".to_string(),
                            date: "unknown".to_string(),
                            sha256: String::new(),
                            reason: Some(format!("Failed to compute SHA-256: {}", e)),
                        });
                        pb.inc(1);
                        continue;
                    }
                };
                if existing_sha256.contains(&sha256) {
                    run_stats.skipped_duplicates += 1;
                    json_report.skipped_duplicates.push(report::JsonImageEntry {
                        source_path: path.display().to_string(),
                        archive_path: None,
                        camera: "unknown".to_string(),
                        date: "unknown".to_string(),
                        sha256: sha256.clone(),
                        reason: Some("Already archived (SHA-256 match)".to_string()),
                    });
                    pb.inc(1);
                    continue;
                }
            }
            run_stats.failed += 1;
            run_stats.failures.push((
                path.display().to_string(),
                "unsupported codec (HEIC)".to_string(),
            ));
            json_report.failed.push(report::JsonImageEntry {
                source_path: path.display().to_string(),
                archive_path: None,
                camera: "unknown".to_string(),
                date: "unknown".to_string(),
                sha256: String::new(),
                reason: Some("unsupported codec".to_string()),
            });
            pb.inc(1);
            continue;
        }

        let sha256 = match dedup::Deduplicator::compute_sha256(path) {
            Ok(h) => h,
            Err(e) => {
                run_stats.failed += 1;
                run_stats.failures.push((path.display().to_string(), e));
                pb.inc(1);
                continue;
            }
        };

        if args.incremental && existing_sha256.contains(&sha256) {
            run_stats.skipped_duplicates += 1;
            json_report.skipped_duplicates.push(report::JsonImageEntry {
                source_path: path.display().to_string(),
                archive_path: None,
                camera: "unknown".to_string(),
                date: "unknown".to_string(),
                sha256: sha256.clone(),
                reason: Some("Already archived (SHA-256 match)".to_string()),
            });
            pb.inc(1);
            continue;
        }

        let phash_val = match dedup::Deduplicator::compute_phash(path) {
            Ok(h) => h,
            Err(_) => 0u64,
        };

        if !args.incremental && existing_sha256.contains(&sha256) {
            run_stats.skipped_duplicates += 1;
            json_report.skipped_duplicates.push(report::JsonImageEntry {
                source_path: path.display().to_string(),
                archive_path: None,
                camera: "unknown".to_string(),
                date: "unknown".to_string(),
                sha256: sha256.clone(),
                reason: Some("Already archived (SHA-256 match)".to_string()),
            });
            pb.inc(1);
            continue;
        }

        let dup_info = dedup.check_duplicate(&sha256, phash_val);

        if dup_info.dup_type == DuplicateType::Exact {
            run_stats.skipped_duplicates += 1;
            json_report.skipped_duplicates.push(report::JsonImageEntry {
                source_path: path.display().to_string(),
                archive_path: None,
                camera: "unknown".to_string(),
                date: "unknown".to_string(),
                sha256: sha256.clone(),
                reason: Some(format!(
                    "Exact duplicate of {}",
                    dup_info.original_path.as_ref().map(|p| p.display().to_string()).unwrap_or_default()
                )),
            });
            pb.inc(1);
            continue;
        }

        let mut metadata = metadata::extract_metadata(path, *format);

        if let Some(ref gps) = metadata.gps {
            if let Some(geo) = geocoder.reverse_geocode(gps) {
                metadata.geo = Some(geo);
            }
        }

        let ext = template::get_file_ext(path);

        let seq_key = format!(
            "{}_{}_{}",
            metadata.date_time.format("%Y%m%d"),
            metadata.camera_model,
            metadata.geo.as_ref().map(|g| format!("{}_{}", g.province, g.city)).unwrap_or_default()
        );
        let seq = seq_counter.entry(seq_key.clone()).or_insert(0);
        *seq += 1;

        let rendered = template::render_template(
            &args.template,
            &metadata,
            &ext,
            *seq,
            args.seq_digits,
            &args.unknown_placeholder,
        );

        let target_path = output.join(&rendered);

        let img_info = ImageInfo {
            path: path.clone(),
            file_size: *file_size,
            format: *format,
            metadata: metadata.clone(),
            phash: phash_val,
            sha256: sha256.clone(),
        };
        all_images.push(img_info);

        if dup_info.dup_type == DuplicateType::Suspected {
            run_stats.suspected_duplicates += 1;

            let dup_dir = output.join("duplicates");
            let dup_target = dup_dir.join(path.file_name().unwrap_or_default());

            if let Some(orig_path) = &dup_info.original_path {
                let orig_phash = dedup.get_phash_for_path(orig_path).unwrap_or(0);
                let dist = crate::phash::hamming_distance(orig_phash, phash_val);
                suspected_dups_with_distance.push((
                    path.file_name().and_then(|f| f.to_str()).unwrap_or("unknown").to_string(),
                    orig_path.display().to_string(),
                    dist,
                ));
            }

            json_report.suspected_duplicates.push(report::JsonImageEntry {
                source_path: path.display().to_string(),
                archive_path: Some(dup_target.display().to_string()),
                camera: metadata.camera_model.clone(),
                date: format!("{}", metadata.date_time.format("%Y-%m-%d")),
                sha256: sha256.clone(),
                reason: Some(format!(
                    "Suspected duplicate of {}",
                    dup_info.original_path.as_ref().map(|p| p.display().to_string()).unwrap_or_default()
                )),
            });

            if !args.dry_run {
                suspected_dup_targets.push((path.clone(), dup_target));
            }

            dedup.register(&sha256, phash_val, path);
            pb.inc(1);
            continue;
        }

        let result = archiver.archive_file(path, &target_path);

        if result.success {
            run_stats.archived += 1;

            if !args.dry_run {
                let index_entry = index::IndexEntry {
                    source_path: path.display().to_string(),
                    archive_path: result.target.display().to_string(),
                    sha256: sha256.clone(),
                    phash: phash_val,
                    archived_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                    date_time: Some(format!("{}", metadata.date_time.format("%Y-%m-%d %H:%M:%S"))),
                    gps: metadata.gps.clone(),
                    geo: metadata.geo.clone(),
                };
                archive_index.add_entry(index_entry);

                undo_record.add_entry(
                    path.display().to_string(),
                    result.target.display().to_string(),
                );
            }

            json_report.archived.push(report::JsonImageEntry {
                source_path: path.display().to_string(),
                archive_path: Some(result.target.display().to_string()),
                camera: metadata.camera_model.clone(),
                date: format!("{}", metadata.date_time.format("%Y-%m-%d")),
                sha256: sha256.clone(),
                reason: None,
            });
        } else {
            run_stats.failed += 1;
            run_stats.failures.push((
                path.display().to_string(),
                result.error.unwrap_or_else(|| "Unknown error".to_string()),
            ));
        }

        dedup.register(&sha256, phash_val, path);
        pb.inc(1);
    }

    pb.finish_with_message("Done");

    if !args.dry_run {
        for (src, dst) in &suspected_dup_targets {
            if let Some(parent) = dst.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::copy(src, dst);
        }

        if let Err(e) = archive_index.save(&output) {
            eprintln!("Warning: Failed to save archive index: {}", e);
        }

        if !undo_record.entries.is_empty() {
            if let Err(e) = undo_record.save(&output) {
                eprintln!("Warning: Failed to save undo record: {}", e);
            }
        }
    }

    run_stats.print_report();

    let need_stats = args.stats || args.report_html.is_some() || args.report_json.is_some();
    if need_stats {
        let stats = report::Stats::from_images(&all_images);

        if args.stats {
            stats.print_stats();
        }

        json_report.by_camera = stats.by_camera;
        json_report.by_year_month = stats.by_year_month;
        json_report.by_format = stats.by_format;
        json_report.resolution_distribution = {
            let mut m = HashMap::new();
            m.insert("4K/UHD".to_string(), stats.resolution_dist.ultra_hd);
            m.insert("1080P".to_string(), stats.resolution_dist.full_hd);
            m.insert("720P".to_string(), stats.resolution_dist.hd);
            m.insert("SD".to_string(), stats.resolution_dist.sd);
            m.insert("Other".to_string(), stats.resolution_dist.other);
            m
        };
        json_report.gps_coverage = report::GpsCoverage {
            with_gps: stats.total_with_gps,
            without_gps: stats.total - stats.total_with_gps,
            percentage: if stats.total > 0 {
                stats.total_with_gps as f64 / stats.total as f64 * 100.0
            } else {
                0.0
            },
        };
    }

    json_report.summary.archived = run_stats.archived;
    json_report.summary.skipped_duplicates = run_stats.skipped_duplicates;
    json_report.summary.suspected_duplicates = run_stats.suspected_duplicates;
    json_report.summary.failed = run_stats.failed;

    if let Some(ref report_path) = args.report_json {
        match json_report.save(report_path) {
            Ok(_) => eprintln!("JSON report saved to: {}", report_path.display()),
            Err(e) => eprintln!("Error saving JSON report: {}", e),
        }
    }

    if let Some(ref report_path) = args.report_html {
        match json_report.save_html(report_path, &suspected_dups_with_distance) {
            Ok(_) => eprintln!("HTML report saved to: {}", report_path.display()),
            Err(e) => eprintln!("Error saving HTML report: {}", e),
        }
    }
}

fn execute_undo(output_dir: &std::path::Path) {
    if !output_dir.exists() {
        eprintln!("Error: Output directory does not exist: {}", output_dir.display());
        std::process::exit(1);
    }

    eprintln!("Loading undo record from: {}", output_dir.display());

    let record = match undo::UndoRecord::load(output_dir) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    eprintln!(
        "Undo record found: {} operations, mode: {}, timestamp: {}",
        record.entries.len(),
        record.mode,
        record.timestamp
    );

    let mut archive_index = index::ArchiveIndex::load(output_dir).unwrap_or_else(|e| {
        eprintln!("Warning: Could not load archive index ({}). Starting fresh.", e);
        index::ArchiveIndex::new()
    });
    let initial_index_len = archive_index.entries.len();

    match record.execute() {
        Ok((restored, failed)) => {
            eprintln!("Undo completed: {} restored, {} failed", restored, failed);

            for entry in &record.entries {
                archive_index.remove_by_archive_path(&entry.archive_path);
            }

            let removed = initial_index_len - archive_index.entries.len();
            if removed > 0 {
                match archive_index.save(output_dir) {
                    Ok(_) => eprintln!("Archive index updated: {} entries removed.", removed),
                    Err(e) => eprintln!("Warning: Failed to update archive index: {}", e),
                }
            }

            if failed == 0 {
                if let Err(e) = undo::UndoRecord::delete(output_dir) {
                    eprintln!("Warning: Failed to delete undo record: {}", e);
                } else {
                    eprintln!("Undo record deleted.");
                }
            } else {
                eprintln!("Some operations failed; undo record preserved for manual inspection.");
            }
        }
        Err(e) => {
            eprintln!("Error during undo: {}", e);
            std::process::exit(1);
        }
    }
}
