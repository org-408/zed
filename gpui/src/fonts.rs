use crate::{
    geometry::vector::{vec2f, Vector2F},
    platform,
};
use anyhow::{anyhow, Result};
use font_kit::metrics::Metrics;
pub use font_kit::properties::{Properties, Weight};
use parking_lot::{RwLock, RwLockUpgradableReadGuard};
use std::{collections::HashMap, sync::Arc};

pub type GlyphId = u32;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct FamilyId(usize);

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct FontId(pub usize);

pub struct FontCache(RwLock<FontCacheState>);

pub struct FontCacheState {
    fonts: Arc<dyn platform::FontSystem>,
    families: Vec<Family>,
    font_selections: HashMap<FamilyId, HashMap<Properties, FontId>>,
    metrics: HashMap<FontId, Metrics>,
}

unsafe impl Send for FontCache {}

struct Family {
    name: String,
    font_ids: Vec<FontId>,
}

impl FontCache {
    pub fn new(fonts: Arc<dyn platform::FontSystem>) -> Self {
        Self(RwLock::new(FontCacheState {
            fonts,
            families: Vec::new(),
            font_selections: HashMap::new(),
            metrics: HashMap::new(),
        }))
    }

    pub fn load_family(&self, names: &[&str]) -> Result<FamilyId> {
        for name in names {
            let state = self.0.upgradable_read();

            if let Some(ix) = state.families.iter().position(|f| f.name == *name) {
                return Ok(FamilyId(ix));
            }

            let mut state = RwLockUpgradableReadGuard::upgrade(state);

            if let Ok(font_ids) = state.fonts.load_family(name) {
                if font_ids.is_empty() {
                    continue;
                }

                let family_id = FamilyId(state.families.len());
                for font_id in &font_ids {
                    if state.fonts.glyph_for_char(*font_id, 'm').is_none() {
                        return Err(anyhow!("font must contain a glyph for the 'm' character"));
                    }
                }

                state.families.push(Family {
                    name: String::from(*name),
                    font_ids,
                });
                return Ok(family_id);
            }
        }

        Err(anyhow!(
            "could not find a non-empty font family matching one of the given names"
        ))
    }

    pub fn default_font(&self, family_id: FamilyId) -> FontId {
        self.select_font(family_id, &Properties::default()).unwrap()
    }

    pub fn select_font(&self, family_id: FamilyId, properties: &Properties) -> Result<FontId> {
        let inner = self.0.upgradable_read();
        if let Some(font_id) = inner
            .font_selections
            .get(&family_id)
            .and_then(|f| f.get(properties))
        {
            Ok(*font_id)
        } else {
            let mut inner = RwLockUpgradableReadGuard::upgrade(inner);
            let family = &inner.families[family_id.0];
            let font_id = inner
                .fonts
                .select_font(&family.font_ids, properties)
                .unwrap_or(family.font_ids[0]);

            inner
                .font_selections
                .entry(family_id)
                .or_default()
                .insert(properties.clone(), font_id);
            Ok(font_id)
        }
    }

    pub fn metric<F, T>(&self, font_id: FontId, f: F) -> T
    where
        F: FnOnce(&Metrics) -> T,
        T: 'static,
    {
        let state = self.0.upgradable_read();
        if let Some(metrics) = state.metrics.get(&font_id) {
            f(metrics)
        } else {
            let metrics = state.fonts.font_metrics(font_id);
            let metric = f(&metrics);
            let mut state = RwLockUpgradableReadGuard::upgrade(state);
            state.metrics.insert(font_id, metrics);
            metric
        }
    }

    pub fn bounding_box(&self, font_id: FontId, font_size: f32) -> Vector2F {
        let bounding_box = self.metric(font_id, |m| m.bounding_box);
        let width = self.scale_metric(bounding_box.width(), font_id, font_size);
        let height = self.scale_metric(bounding_box.height(), font_id, font_size);
        vec2f(width, height)
    }

    pub fn em_width(&self, font_id: FontId, font_size: f32) -> f32 {
        let state = self.0.read();
        let glyph_id = state.fonts.glyph_for_char(font_id, 'm').unwrap();
        let bounds = state.fonts.typographic_bounds(font_id, glyph_id).unwrap();
        self.scale_metric(bounds.width(), font_id, font_size)
    }

    pub fn line_height(&self, font_id: FontId, font_size: f32) -> f32 {
        let bounding_box = self.metric(font_id, |m| m.bounding_box);
        self.scale_metric(bounding_box.height(), font_id, font_size)
    }

    pub fn cap_height(&self, font_id: FontId, font_size: f32) -> f32 {
        self.scale_metric(self.metric(font_id, |m| m.cap_height), font_id, font_size)
    }

    pub fn ascent(&self, font_id: FontId, font_size: f32) -> f32 {
        self.scale_metric(self.metric(font_id, |m| m.ascent), font_id, font_size)
    }

    pub fn descent(&self, font_id: FontId, font_size: f32) -> f32 {
        self.scale_metric(self.metric(font_id, |m| m.descent), font_id, font_size)
    }

    pub fn scale_metric(&self, metric: f32, font_id: FontId, font_size: f32) -> f32 {
        metric * font_size / self.metric(font_id, |m| m.units_per_em as f32)
    }
}
