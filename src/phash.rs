use rustdct::DctPlanner;

const DCT_SIZE: usize = 32;
const HASH_SIZE: usize = 8;

pub fn compute_phash(path: &std::path::Path) -> Result<u64, String> {
    let img = image::open(path)
        .map_err(|e| format!("Failed to open image: {}", e))?;

    let resized = image::imageops::resize(
        &img,
        DCT_SIZE as u32,
        DCT_SIZE as u32,
        image::imageops::FilterType::Lanczos3,
    );
    let gray_buffer = image::imageops::grayscale(&resized);

    let mut matrix = vec![0.0f64; DCT_SIZE * DCT_SIZE];
    for y in 0..DCT_SIZE {
        for x in 0..DCT_SIZE {
            matrix[y * DCT_SIZE + x] = gray_buffer.get_pixel(x as u32, y as u32).0[0] as f64;
        }
    }

    let mut planner = DctPlanner::new();
    let dct = planner.plan_dct2(DCT_SIZE);

    let mut temp = vec![0.0f64; DCT_SIZE];
    for y in 0..DCT_SIZE {
        let start = y * DCT_SIZE;
        temp.copy_from_slice(&matrix[start..start + DCT_SIZE]);
        dct.process_dct2(&mut temp);
        matrix[start..start + DCT_SIZE].copy_from_slice(&temp);
    }

    let mut transposed = vec![0.0f64; DCT_SIZE * DCT_SIZE];
    transpose::transpose(&matrix, &mut transposed, DCT_SIZE, DCT_SIZE);

    for y in 0..DCT_SIZE {
        let start = y * DCT_SIZE;
        temp.copy_from_slice(&transposed[start..start + DCT_SIZE]);
        dct.process_dct2(&mut temp);
        transposed[start..start + DCT_SIZE].copy_from_slice(&temp);
    }

    transpose::transpose(&transposed, &mut matrix, DCT_SIZE, DCT_SIZE);

    let mut values: Vec<f64> = Vec::with_capacity(HASH_SIZE * HASH_SIZE - 1);
    for r in 0..HASH_SIZE {
        for c in 0..HASH_SIZE {
            if r == 0 && c == 0 {
                continue;
            }
            values.push(matrix[r * DCT_SIZE + c]);
        }
    }

    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = values[values.len() / 2];

    let mut hash: u64 = 0;
    let mut bit_index = 0u64;
    for r in 0..HASH_SIZE {
        for c in 0..HASH_SIZE {
            if r == 0 && c == 0 {
                continue;
            }
            if matrix[r * DCT_SIZE + c] >= median {
                hash |= 1u64 << bit_index;
            }
            bit_index += 1;
        }
    }

    Ok(hash)
}

pub fn hamming_distance(hash1: u64, hash2: u64) -> u32 {
    (hash1 ^ hash2).count_ones()
}
