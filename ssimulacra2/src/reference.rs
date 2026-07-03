use yuvxyb::{LinearRgb, Xyb};

use crate::blur::Blur;
use crate::{
    Msssim, MsssimScale, NUM_SCALES, Ssimulacra2Error, downscale_by_2, edge_diff_map,
    image_multiply_owned, join, make_positive_xyb, ssim_map, xyb_to_planar,
};

/// Pyramide précalculée de l'image de référence.
///
/// Dans une recherche de qualité, la référence est fixe pendant que le candidat
/// change à chaque itération : précalculer son côté du travail (downscales,
/// conversion XYB, `mu1`, `sigma1_sq`) évite d'en payer ~45 % à chaque score.
#[derive(Debug)]
pub struct ReferenceFrame {
    scales: Vec<RefScale>,
}

#[derive(Debug)]
struct RefScale {
    width: usize,
    height: usize,
    planar: [Vec<f32>; 3],
    mu1: [Vec<f32>; 3],
    sigma1_sq: [Vec<f32>; 3],
}

impl ReferenceFrame {
    /// Précalcule la pyramide de la référence.
    ///
    /// # Errors
    /// - If the source image cannot be converted to XYB successfully
    /// - If the image is smaller than 8x8 pixels
    pub fn new<T>(source: T) -> Result<Self, Ssimulacra2Error>
    where
        LinearRgb: TryFrom<T>,
    {
        let Ok(img) = LinearRgb::try_from(source) else {
            return Err(Ssimulacra2Error::LinearRgbConversionFailed);
        };
        if img.width() < 8 || img.height() < 8 {
            return Err(Ssimulacra2Error::InvalidImageSize);
        }
        Ok(Self::from_linear(img))
    }

    pub(crate) fn from_linear(mut img1: LinearRgb) -> Self {
        let mut width = img1.width();
        let mut height = img1.height();
        let mut blur = Blur::new(width, height);
        let mut scales = Vec::with_capacity(NUM_SCALES);

        for scale in 0..NUM_SCALES {
            if width < 8 || height < 8 {
                break;
            }
            if scale > 0 {
                img1 = downscale_by_2(&img1);
                width = img1.width();
                height = img1.height();
            }
            blur.shrink_to(width, height);

            let mut xyb = Xyb::from(img1.clone());
            make_positive_xyb(&mut xyb);
            let planar = xyb_to_planar(&xyb);

            let mul = image_multiply_owned(&planar, &planar);
            let (sigma1_sq, mu1) =
                join(|| blur.blur_parallel(&mul), || blur.blur_parallel(&planar));

            scales.push(RefScale {
                width,
                height,
                planar,
                mu1,
                sigma1_sq,
            });
        }
        Self { scales }
    }

    #[must_use]
    pub fn width(&self) -> usize {
        self.scales[0].width
    }

    #[must_use]
    pub fn height(&self) -> usize {
        self.scales[0].height
    }

    /// Score du candidat contre cette référence.
    ///
    /// # Errors
    /// - If the distorted image cannot be converted to XYB successfully
    /// - If the dimensions do not match the reference
    pub fn score<U>(&self, distorted: U) -> Result<f64, Ssimulacra2Error>
    where
        LinearRgb: TryFrom<U>,
    {
        let Ok(img) = LinearRgb::try_from(distorted) else {
            return Err(Ssimulacra2Error::LinearRgbConversionFailed);
        };
        if img.width() != self.width() || img.height() != self.height() {
            return Err(Ssimulacra2Error::NonMatchingImageDimensions);
        }
        Ok(self.score_linear(img))
    }

    pub(crate) fn score_linear(&self, mut img2: LinearRgb) -> f64 {
        let mut blur = Blur::new(self.width(), self.height());
        let mut msssim = Msssim::default();

        for (scale, r) in self.scales.iter().enumerate() {
            if scale > 0 {
                img2 = downscale_by_2(&img2);
            }
            blur.shrink_to(r.width, r.height);

            let mut xyb = Xyb::from(img2.clone());
            make_positive_xyb(&mut xyb);
            let planar2 = xyb_to_planar(&xyb);

            let (mul22, mul12) = join(
                || image_multiply_owned(&planar2, &planar2),
                || image_multiply_owned(&r.planar, &planar2),
            );
            let ((sigma2_sq, sigma12), mu2) = join(
                || {
                    join(
                        || blur.blur_parallel(&mul22),
                        || blur.blur_parallel(&mul12),
                    )
                },
                || blur.blur_parallel(&planar2),
            );

            let avg_ssim = ssim_map(
                r.width,
                r.height,
                &r.mu1,
                &mu2,
                &r.sigma1_sq,
                &sigma2_sq,
                &sigma12,
            );
            let avg_edgediff = edge_diff_map(r.width, r.height, &r.planar, &r.mu1, &planar2, &mu2);
            msssim.scales.push(MsssimScale {
                avg_ssim,
                avg_edgediff,
            });
        }
        msssim.score()
    }
}
