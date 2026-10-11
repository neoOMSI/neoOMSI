//! A picture's buffer writes gathered into one staging buffer.
use crate::*;

#[derive(Default)]
pub(crate) struct Uploads {
    open: bool,
    data: Vec<u8>,
    copies: Vec<(wgpu::Buffer, u64, u64, u64)>,
}

/// `queue.write_buffer`, gathered while a picture's writes are being collected (the fields
/// apart, for code that holds another field of the renderer).
pub(crate) fn write_gathered(
    uploads: &std::cell::RefCell<Uploads>,
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    offset: u64,
    data: &[u8],
) {
    let mut u = uploads.borrow_mut();
    let aligned = offset % wgpu::COPY_BUFFER_ALIGNMENT == 0
        && data.len() as u64 % wgpu::COPY_BUFFER_ALIGNMENT == 0;
    if !u.open || !aligned || data.is_empty() {
        drop(u);
        queue.write_buffer(buffer, offset, data);
        return;
    }
    let at = u.data.len() as u64;
    u.data.extend_from_slice(data);
    u.copies.push((buffer.clone(), offset, at, data.len() as u64));
}

impl Renderer {
    /// `queue.write_buffer`, gathered while a picture's writes are being collected.
    pub(crate) fn write_buffer(&self, buffer: &wgpu::Buffer, offset: u64, data: &[u8]) {
        write_gathered(&self.uploads, &self.queue, buffer, offset, data);
    }

    /// From here on the picture's writes are gathered (left over ones are written at once).
    pub(crate) fn open_uploads(&self) {
        self.write_uploads_now();
        self.uploads.borrow_mut().open = true;
    }

    fn write_uploads_now(&self) {
        let mut u = self.uploads.borrow_mut();
        u.open = false;
        let Uploads { data, copies, .. } = &mut *u;
        for (buffer, offset, at, len) in copies.drain(..) {
            self.queue
                .write_buffer(&buffer, offset, &data[at as usize..(at + len) as usize]);
        }
        data.clear();
    }

    /// Copies the gathered writes at the start of `encoder` (the first one submitted) and
    /// stops gathering.
    pub(crate) fn flush_uploads(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut u = self.uploads.borrow_mut();
        u.open = false;
        if u.copies.is_empty() {
            u.data.clear();
            return;
        }
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("picture uploads"),
            size: u.data.len() as u64,
            usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: true,
        });
        let mapped = match staging.slice(..).get_mapped_range_mut() {
            Ok(mut view) => {
                let len = u.data.len();
                view.slice(0..len).copy_from_slice(&u.data);
                true
            }
            Err(_) => false,
        };
        if !mapped {
            drop(u);
            self.write_uploads_now();
            return;
        }
        staging.unmap();
        let Uploads { data, copies, .. } = &mut *u;
        for (buffer, offset, at, len) in copies.drain(..) {
            encoder.copy_buffer_to_buffer(&staging, at, &buffer, offset, len);
        }
        data.clear();
    }
}
