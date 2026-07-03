mod consts {
    #![allow(clippy::unreadable_literal)]
    include!(concat!(env!("OUT_DIR"), "/recursive_gaussian.rs"));
}

/// Implements "Recursive Implementation of the Gaussian Filter Using Truncated
/// Cosine Functions" by Charalampidis [2016].
pub struct RecursiveGaussian;

impl RecursiveGaussian {
    #[cfg(feature = "rayon")]
    pub fn horizontal_pass(&self, input: &[f32], output: &mut [f32], width: usize) {
        use rayon::iter::{IndexedParallelIterator, ParallelIterator};
        use rayon::prelude::ParallelSliceMut;
        use rayon::slice::ParallelSlice;

        assert_eq!(input.len(), output.len());

        input
            .par_chunks_exact(width)
            .zip(output.par_chunks_exact_mut(width))
            .for_each(|(input, output)| self.horizontal_row(input, output, width));
    }

    #[cfg(not(feature = "rayon"))]
    pub fn horizontal_pass(&self, input: &[f32], output: &mut [f32], width: usize) {
        assert_eq!(input.len(), output.len());

        for (input, output) in input
            .chunks_exact(width)
            .zip(output.chunks_exact_mut(width))
        {
            self.horizontal_row(input, output, width);
        }
    }

    fn horizontal_row(&self, input: &[f32], output: &mut [f32], width: usize) {
        let big_n = consts::RADIUS as isize;
        let mut prev_1 = 0f32;
        let mut prev_3 = 0f32;
        let mut prev_5 = 0f32;
        let mut prev2_1 = 0f32;
        let mut prev2_3 = 0f32;
        let mut prev2_5 = 0f32;

        let mut n = (-big_n) + 1;
        while n < width as isize {
            let left = n - big_n - 1;
            let right = n + big_n - 1;
            let left_val = if left >= 0 {
                // SAFETY: `left` can never be bigger than `width`
                unsafe { *input.get_unchecked(left as usize) }
            } else {
                0f32
            };
            let right_val = if right < width as isize {
                // SAFETY: this branch ensures that `right` is not bigger than `width`
                unsafe { *input.get_unchecked(right as usize) }
            } else {
                0f32
            };
            let sum = left_val + right_val;

            let mut out_1 = sum * consts::MUL_IN_1;
            let mut out_3 = sum * consts::MUL_IN_3;
            let mut out_5 = sum * consts::MUL_IN_5;

            out_1 = consts::MUL_PREV2_1.mul_add(prev2_1, out_1);
            out_3 = consts::MUL_PREV2_3.mul_add(prev2_3, out_3);
            out_5 = consts::MUL_PREV2_5.mul_add(prev2_5, out_5);
            prev2_1 = prev_1;
            prev2_3 = prev_3;
            prev2_5 = prev_5;

            out_1 = consts::MUL_PREV_1.mul_add(prev_1, out_1);
            out_3 = consts::MUL_PREV_3.mul_add(prev_3, out_3);
            out_5 = consts::MUL_PREV_5.mul_add(prev_5, out_5);
            prev_1 = out_1;
            prev_3 = out_3;
            prev_5 = out_5;

            if n >= 0 {
                // SAFETY: We know that this chunk of output is of size `width`,
                // which `n` cannot be larger than.
                unsafe {
                    *output.get_unchecked_mut(n as usize) = out_1 + out_3 + out_5;
                }
            }

            n += 1;
        }
    }

    pub fn vertical_pass_chunked<const J: usize, const K: usize>(
        &self,
        input: &[f32],
        output: &mut [f32],
        width: usize,
        height: usize,
    ) {
        assert!(J > K);
        assert!(K > 0);

        assert_eq!(input.len(), output.len());

        let mut x = 0;
        while x + J <= width {
            self.vertical_pass::<J>(&input[x..], &mut output[x..], width, height);
            x += J;
        }

        while x + K <= width {
            self.vertical_pass::<K>(&input[x..], &mut output[x..], width, height);
            x += K;
        }

        while x < width {
            self.vertical_pass::<1>(&input[x..], &mut output[x..], width, height);
            x += 1;
        }
    }

    // Comme vertical_pass_chunked, mais chaque bande de colonnes est traitée en
    // parallèle dans un buffer contigu par bande, puis recopiée (scatter) vers
    // la sortie strided. Les valeurs par colonne sont identiques au chemin
    // séquentiel ; seul l'ordre d'exécution des bandes change.
    #[cfg(feature = "rayon")]
    pub fn vertical_pass_parallel<const J: usize, const K: usize>(
        &self,
        input: &[f32],
        output: &mut [f32],
        width: usize,
        height: usize,
    ) {
        use rayon::prelude::*;

        assert!(J > K);
        assert!(K > 0);
        assert_eq!(input.len(), output.len());

        let mut stripes = Vec::new();
        let mut x = 0;
        while x + J <= width {
            stripes.push((x, J));
            x += J;
        }
        while x + K <= width {
            stripes.push((x, K));
            x += K;
        }
        while x < width {
            stripes.push((x, 1));
            x += 1;
        }

        let bufs: Vec<Vec<f32>> = stripes
            .par_iter()
            .map(|&(x0, cols)| {
                let mut buf = vec![0f32; cols * height];
                match cols {
                    c if c == J => {
                        self.vertical_pass_to::<J>(&input[x0..], &mut buf, width, height);
                    }
                    c if c == K => {
                        self.vertical_pass_to::<K>(&input[x0..], &mut buf, width, height);
                    }
                    _ => {
                        self.vertical_pass_to::<1>(&input[x0..], &mut buf, width, height);
                    }
                }
                buf
            })
            .collect();

        output
            .par_chunks_mut(width)
            .enumerate()
            .for_each(|(y, row)| {
                for (i, &(x0, cols)) in stripes.iter().enumerate() {
                    row[x0..x0 + cols].copy_from_slice(&bufs[i][y * cols..(y + 1) * cols]);
                }
            });
    }

    // Variante de vertical_pass écrivant dans un buffer contigu de COLUMNS
    // colonnes (out stride = COLUMNS) au lieu de la sortie strided.
    #[cfg(feature = "rayon")]
    fn vertical_pass_to<const COLUMNS: usize>(
        &self,
        input: &[f32],
        output: &mut [f32],
        width: usize,
        height: usize,
    ) {
        let big_n = consts::RADIUS as isize;

        let zeroes = vec![0f32; COLUMNS];
        let mut prev = vec![0f32; 3 * COLUMNS];
        let mut prev2 = vec![0f32; 3 * COLUMNS];
        let mut out = vec![0f32; 3 * COLUMNS];

        let mut n = (-big_n) + 1;
        while n < height as isize {
            let top = n - big_n - 1;
            let bottom = n + big_n - 1;
            let top_row = if top >= 0 {
                &input[top as usize * width..][..COLUMNS]
            } else {
                &zeroes
            };

            let bottom_row = if bottom < height as isize {
                &input[bottom as usize * width..][..COLUMNS]
            } else {
                &zeroes
            };

            for i in 0..COLUMNS {
                let sum = top_row[i] + bottom_row[i];

                let i1 = i;
                let i3 = i1 + COLUMNS;
                let i5 = i3 + COLUMNS;

                let out1 = prev[i1].mul_add(consts::VERT_MUL_PREV_1, prev2[i1]);
                let out3 = prev[i3].mul_add(consts::VERT_MUL_PREV_3, prev2[i3]);
                let out5 = prev[i5].mul_add(consts::VERT_MUL_PREV_5, prev2[i5]);

                let out1 = sum.mul_add(consts::VERT_MUL_IN_1, -out1);
                let out3 = sum.mul_add(consts::VERT_MUL_IN_3, -out3);
                let out5 = sum.mul_add(consts::VERT_MUL_IN_5, -out5);

                out[i1] = out1;
                out[i3] = out3;
                out[i5] = out5;

                if n >= 0 {
                    output[n as usize * COLUMNS + i] = out1 + out3 + out5;
                }
            }

            prev2.copy_from_slice(&prev);
            prev.copy_from_slice(&out);

            n += 1;
        }
    }

    // Apply 1D vertical scan on COLUMNS elements at a time
    pub fn vertical_pass<const COLUMNS: usize>(
        &self,
        input: &[f32],
        output: &mut [f32],
        width: usize,
        height: usize,
    ) {
        assert_eq!(input.len(), output.len());

        let big_n = consts::RADIUS as isize;

        let zeroes = vec![0f32; COLUMNS];
        let mut prev = vec![0f32; 3 * COLUMNS];
        let mut prev2 = vec![0f32; 3 * COLUMNS];
        let mut out = vec![0f32; 3 * COLUMNS];

        let mut n = (-big_n) + 1;
        while n < height as isize {
            let top = n - big_n - 1;
            let bottom = n + big_n - 1;
            let top_row = if top >= 0 {
                &input[top as usize * width..][..COLUMNS]
            } else {
                &zeroes
            };

            let bottom_row = if bottom < height as isize {
                &input[bottom as usize * width..][..COLUMNS]
            } else {
                &zeroes
            };

            for i in 0..COLUMNS {
                let sum = top_row[i] + bottom_row[i];

                let i1 = i;
                let i3 = i1 + COLUMNS;
                let i5 = i3 + COLUMNS;

                let out1 = prev[i1].mul_add(consts::VERT_MUL_PREV_1, prev2[i1]);
                let out3 = prev[i3].mul_add(consts::VERT_MUL_PREV_3, prev2[i3]);
                let out5 = prev[i5].mul_add(consts::VERT_MUL_PREV_5, prev2[i5]);

                let out1 = sum.mul_add(consts::VERT_MUL_IN_1, -out1);
                let out3 = sum.mul_add(consts::VERT_MUL_IN_3, -out3);
                let out5 = sum.mul_add(consts::VERT_MUL_IN_5, -out5);

                out[i1] = out1;
                out[i3] = out3;
                out[i5] = out5;

                if n >= 0 {
                    output[n as usize * width + i] = out1 + out3 + out5;
                }
            }

            prev2.copy_from_slice(&prev);
            prev.copy_from_slice(&out);

            n += 1;
        }
    }
}
