mod gaussian;

use gaussian::RecursiveGaussian;

/// Structure handling image blur.
///
/// This struct contains the necessary buffers and the kernel used for blurring
/// (currently a recursive approximation of the Gaussian filter).
///
/// Note that the width and height of the image passed to [blur][Self::blur] needs to exactly
/// match the width and height of this instance. If you reduce the image size (e.g. via
/// downscaling), [`shrink_to`][Self::shrink_to] can be used to resize the internal buffers.
pub struct Blur {
    kernel: RecursiveGaussian,
    temp: Vec<f32>,
    width: usize,
    height: usize,
}

impl Blur {
    /// Create a new [Blur] for images of the given width and height.
    /// This pre-allocates the necessary buffers.
    #[must_use]
    pub fn new(width: usize, height: usize) -> Self {
        Blur {
            kernel: RecursiveGaussian,
            temp: vec![0.0f32; width * height],
            width,
            height,
        }
    }

    /// Truncates the internal buffers to fit images of the given width and height.
    ///
    /// This will [truncate][Vec::truncate] the internal buffers
    /// without affecting the allocated memory.
    pub fn shrink_to(&mut self, width: usize, height: usize) {
        self.temp.truncate(width * height);
        self.width = width;
        self.height = height;
    }

    /// Blur the given image.
    pub fn blur(&mut self, img: &[Vec<f32>; 3]) -> [Vec<f32>; 3] {
        [
            self.blur_plane(&img[0]),
            self.blur_plane(&img[1]),
            self.blur_plane(&img[2]),
        ]
    }

    fn blur_plane(&mut self, plane: &[f32]) -> Vec<f32> {
        let mut out = vec![0f32; self.width * self.height];
        self.kernel
            .horizontal_pass(plane, &mut self.temp, self.width);
        self.kernel
            .vertical_pass_chunked::<128, 32>(&self.temp, &mut out, self.width, self.height);
        out
    }

    /// Comme [blur][Self::blur] mais avec des buffers locaux par plan, ce qui
    /// permet `&self` et l'exécution concurrente des trois plans.
    #[must_use]
    pub fn blur_parallel(&self, img: &[Vec<f32>; 3]) -> [Vec<f32>; 3] {
        let (p0, (p1, p2)) = crate::join(
            || self.blur_plane_local(&img[0]),
            || {
                crate::join(
                    || self.blur_plane_local(&img[1]),
                    || self.blur_plane_local(&img[2]),
                )
            },
        );
        [p0, p1, p2]
    }

    fn blur_plane_local(&self, plane: &[f32]) -> Vec<f32> {
        let mut temp = vec![0f32; self.width * self.height];
        let mut out = vec![0f32; self.width * self.height];
        self.kernel.horizontal_pass(plane, &mut temp, self.width);
        #[cfg(feature = "rayon")]
        self.kernel
            .vertical_pass_parallel::<128, 32>(&temp, &mut out, self.width, self.height);
        #[cfg(not(feature = "rayon"))]
        self.kernel
            .vertical_pass_chunked::<128, 32>(&temp, &mut out, self.width, self.height);
        out
    }
}
