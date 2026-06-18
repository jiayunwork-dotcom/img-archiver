use crate::geo_data;
use crate::types::{GeoLocation, GpsLocation};
use std::collections::HashMap;

pub struct GeoCoder {
    regions: Vec<geo_data::GeoRegion>,
    cache: HashMap<(i64, i64), Option<GeoLocation>>,
}

impl GeoCoder {
    pub fn new() -> Self {
        Self {
            regions: geo_data::get_china_regions(),
            cache: HashMap::new(),
        }
    }

    pub fn reverse_geocode(&mut self, gps: &GpsLocation) -> Option<GeoLocation> {
        let cache_key = (
            (gps.longitude * 100.0).round() as i64,
            (gps.latitude * 100.0).round() as i64,
        );

        if let Some(cached) = self.cache.get(&cache_key) {
            return cached.clone();
        }

        let result = self.find_region(gps.longitude, gps.latitude);
        self.cache.insert(cache_key, result.clone());
        result
    }

    fn find_region(&self, lon: f64, lat: f64) -> Option<GeoLocation> {
        for region in &self.regions {
            if point_in_polygon(lon, lat, &region.boundary) {
                return Some(GeoLocation {
                    province: region.province.clone(),
                    city: region.city.clone(),
                    district: region.district.clone(),
                });
            }
        }

        if is_in_china_bounds(lon, lat) {
            self.find_nearest_region(lon, lat)
        } else {
            None
        }
    }

    fn find_nearest_region(&self, lon: f64, lat: f64) -> Option<GeoLocation> {
        let mut min_dist = f64::MAX;
        let mut nearest: Option<&geo_data::GeoRegion> = None;

        for region in &self.regions {
            let centroid = compute_centroid(&region.boundary);
            let dist = haversine_distance(lat, lon, centroid[1], centroid[0]);
            if dist < min_dist {
                min_dist = dist;
                nearest = Some(region);
            }
        }

        nearest.map(|r| GeoLocation {
            province: r.province.clone(),
            city: r.city.clone(),
            district: r.district.clone(),
        })
    }
}

fn point_in_polygon(lon: f64, lat: f64, polygon: &[[f64; 2]]) -> bool {
    if polygon.len() < 3 {
        return false;
    }

    let mut inside = false;
    let n = polygon.len();
    let mut j = n - 1;

    for i in 0..n {
        let xi = polygon[i][0];
        let yi = polygon[i][1];
        let xj = polygon[j][0];
        let yj = polygon[j][1];

        if ((yi > lat) != (yj > lat))
            && (lon < (xj - xi) * (lat - yi) / (yj - yi) + xi)
        {
            inside = !inside;
        }

        j = i;
    }

    inside
}

fn is_in_china_bounds(lon: f64, lat: f64) -> bool {
    lon >= 73.0 && lon <= 135.0 && lat >= 3.0 && lat <= 54.0
}

fn compute_centroid(polygon: &[[f64; 2]]) -> [f64; 2] {
    if polygon.is_empty() {
        return [0.0, 0.0];
    }
    let n = polygon.len() as f64;
    let sum_lon: f64 = polygon.iter().map(|p| p[0]).sum();
    let sum_lat: f64 = polygon.iter().map(|p| p[1]).sum();
    [sum_lon / n, sum_lat / n]
}

fn haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6371.0;
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    r * c
}
