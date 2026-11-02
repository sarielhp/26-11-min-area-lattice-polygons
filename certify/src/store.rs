//! Completed layers are written once, sequentially, and read back to
//! reconstruct witnesses or to resume an interrupted run. Per layer m:
//! `layer_m.bin` (offsets and entries per height), `layer_m.idx` (first height
//! and byte positions), and `summary_m.bin` (minimum cost per endpoint).
use crate::Error;
use crate::frontier::Packed;
use crate::layer::{HeightRow, Layer};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::os::unix::fs::FileExt;
use std::path::PathBuf;

struct Saved {
    first_h: usize,
    // (byte position of offsets, byte position of entries) per height.
    rows: Vec<(u64, u64)>,
}

pub struct Spill {
    dir: PathBuf,
    layers: Vec<Option<Saved>>,
    pub bytes: u64,
}

fn u32_at(bytes: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]])
}

fn u64_at(bytes: &[u8], i: usize) -> u64 {
    let mut b = [0; 8];
    b.copy_from_slice(&bytes[i..i + 8]);
    u64::from_le_bytes(b)
}

impl Spill {
    /// A fresh run: any old checkpoint in `dir` is discarded.
    pub fn new(dir: PathBuf) -> Result<Self, Error> {
        std::fs::create_dir_all(&dir)?;
        let checkpoint = dir.join(crate::checkpoint::FILE);
        if checkpoint.exists() {
            std::fs::remove_file(checkpoint)?;
        }
        Ok(Self {
            dir,
            layers: Vec::new(),
            bytes: 0,
        })
    }

    /// Reopens layers 2..=last written by an interrupted run.
    pub fn reopen(dir: PathBuf, last: usize) -> Result<Self, Error> {
        let mut spill = Self {
            dir,
            layers: Vec::new(),
            bytes: 0,
        };
        spill.layers.resize_with(last + 1, || None);
        for m in 2..=last {
            let index = std::fs::read(spill.index_path(m))?;
            let first_h = u64_at(&index, 0) as usize;
            let rows = (8..index.len())
                .step_by(16)
                .map(|i| (u64_at(&index, i), u64_at(&index, i + 8)))
                .collect();
            spill.bytes += std::fs::metadata(spill.path(m))?.len();
            spill.layers[m] = Some(Saved { first_h, rows });
        }
        Ok(spill)
    }

    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    fn path(&self, m: usize) -> PathBuf {
        self.dir.join(format!("layer_{m:04}.bin"))
    }

    fn index_path(&self, m: usize) -> PathBuf {
        self.dir.join(format!("layer_{m:04}.idx"))
    }

    fn summary_path(&self, m: usize) -> PathBuf {
        self.dir.join(format!("summary_{m:04}.bin"))
    }

    pub fn write_summary(&self, m: usize, summary: &[u32]) -> Result<(), Error> {
        let bytes: Vec<u8> = summary.iter().flat_map(|v| v.to_le_bytes()).collect();
        std::fs::write(self.summary_path(m), bytes)?;
        Ok(())
    }

    pub fn read_summary(&self, m: usize) -> Result<Vec<u32>, Error> {
        let bytes = std::fs::read(self.summary_path(m))?;
        Ok((0..bytes.len()).step_by(4).map(|i| u32_at(&bytes, i)).collect())
    }

    /// Reads a whole saved layer back into memory.
    pub fn load_layer(&self, m: usize) -> Result<Layer, Error> {
        let saved = self.layers.get(m).and_then(Option::as_ref).ok_or("layer not saved")?;
        let bytes = std::fs::read(self.path(m))?;
        let mut rows = Vec::with_capacity(saved.rows.len());
        for (i, &(offsets_at, entries_at)) in saved.rows.iter().enumerate() {
            let height = saved.first_h + i;
            let offsets: Vec<u32> = (0..=height)
                .map(|x| u32_at(&bytes, offsets_at as usize + 4 * x))
                .collect();
            let count = offsets[height] as usize;
            let entries = (0..count)
                .map(|e| Packed::from_bytes(&bytes[entries_at as usize + 8 * e..]))
                .collect();
            rows.push(HeightRow { offsets, entries });
        }
        Ok(Layer {
            m,
            first_h: saved.first_h,
            rows,
        })
    }

    pub fn write(&mut self, layer: &Layer) -> Result<(), Error> {
        let mut out = BufWriter::with_capacity(1 << 20, File::create(self.path(layer.m))?);
        let mut position = 0_u64;
        let mut rows = Vec::with_capacity(layer.rows.len());
        for row in &layer.rows {
            let offsets_at = position;
            for &offset in &row.offsets {
                out.write_all(&offset.to_le_bytes())?;
            }
            position += 4 * row.offsets.len() as u64;
            let entries_at = position;
            for &entry in &row.entries {
                out.write_all(&entry.to_bytes())?;
            }
            position += 8 * row.entries.len() as u64;
            rows.push((offsets_at, entries_at));
        }
        out.flush()?;
        let mut index = Vec::with_capacity(8 + 16 * rows.len());
        index.extend_from_slice(&(layer.first_h as u64).to_le_bytes());
        for &(offsets_at, entries_at) in &rows {
            index.extend_from_slice(&offsets_at.to_le_bytes());
            index.extend_from_slice(&entries_at.to_le_bytes());
        }
        std::fs::write(self.index_path(layer.m), index)?;
        self.bytes += position;
        if self.layers.len() <= layer.m {
            self.layers.resize_with(layer.m + 1, || None);
        }
        self.layers[layer.m] = Some(Saved {
            first_h: layer.first_h,
            rows,
        });
        Ok(())
    }

    pub fn frontier(&self, m: usize, h: usize, x: usize) -> Result<Vec<Packed>, Error> {
        let saved = self
            .layers
            .get(m)
            .and_then(Option::as_ref)
            .ok_or("layer not saved")?;
        let &(offsets_at, entries_at) = h
            .checked_sub(saved.first_h)
            .and_then(|i| saved.rows.get(i))
            .ok_or("height not saved")?;
        let file = File::open(self.path(m))?;
        let mut pair = [0_u8; 8];
        file.read_exact_at(&mut pair, offsets_at + 4 * x as u64)?;
        let start = u32::from_le_bytes([pair[0], pair[1], pair[2], pair[3]]) as u64;
        let end = u32::from_le_bytes([pair[4], pair[5], pair[6], pair[7]]) as u64;
        let mut bytes = vec![0_u8; 8 * (end - start) as usize];
        file.read_exact_at(&mut bytes, entries_at + 8 * start)?;
        Ok(bytes.chunks_exact(8).map(Packed::from_bytes).collect())
    }

    pub fn remove(self) -> Result<(), Error> {
        std::fs::remove_dir_all(&self.dir)?;
        Ok(())
    }
}
