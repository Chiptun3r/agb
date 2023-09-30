use core::{marker::PhantomData, mem::size_of, pin::Pin};

use alloc::boxed::Box;

use crate::memory_mapped::MemoryMapped;

#[non_exhaustive]
pub struct DmaController {}

impl DmaController {
    pub(crate) const fn new() -> Self {
        Self {}
    }

    pub fn dma(&mut self) -> Dmas<'_> {
        unsafe { Dmas::new() }
    }
}

pub struct Dmas<'gba> {
    phantom: PhantomData<&'gba ()>,

    pub dma0: Dma,
    pub dma3: Dma,
}

impl<'gba> Dmas<'gba> {
    unsafe fn new() -> Self {
        Self {
            phantom: PhantomData,

            dma0: Dma::new(0),
            dma3: Dma::new(3),
        }
    }
}

pub struct Dma {
    number: usize,

    source_addr: MemoryMapped<u32>,
    dest_addr: MemoryMapped<u32>,
    ctrl_addr: MemoryMapped<u32>,
}

impl Dma {
    unsafe fn new(number: usize) -> Self {
        Self {
            number,
            source_addr: unsafe { MemoryMapped::new(dma_source_addr(number)) },
            dest_addr: unsafe { MemoryMapped::new(dma_dest_addr(number)) },
            ctrl_addr: unsafe { MemoryMapped::new(dma_control_addr(number)) },
        }
    }

    fn disable(&mut self) {
        unsafe { MemoryMapped::new(dma_control_addr(self.number)) }.set(0);
    }

    pub unsafe fn hblank_transfer<'a, T>(
        &'a self,
        location: &DmaControllable<T>,
        values: &[T],
    ) -> DmaTransferHandle<'a, T>
    where
        T: Copy,
    {
        assert!(
            values.len() >= 160,
            "need to pass at least 160 values for a hblank_transfer"
        );
        let handle = unsafe { DmaTransferHandle::new(self.number, values) };

        let n_transfers = (size_of::<T>() / 2) as u32;

        self.source_addr.set(handle.data.as_ptr() as u32);
        self.dest_addr.set(location.memory_location as u32);

        self.ctrl_addr.set(
            (0b10 << 0x15) | // keep destination address fixed
            // (0b00 << 0x17) | // increment the source address each time
            1 << 0x19 | // repeat the copy each hblank
            // 0 << 0x1a | // copy in half words (see n_transfers above)
            0b10 << 0x1c | // copy each hblank
            1 << 0x1f | // enable the dma
            n_transfers, // the number of halfwords to copy
        );

        handle
    }
}

/// A struct to describe things you can modify using DMA (normally some register within the GBA)
///
/// This is generally used to perform fancy graphics tricks like screen wobble on a per-scanline basis or
/// to be able to create a track like in mario kart. This is an advanced technique and likely not needed
/// unless you want to do fancy graphics.
pub struct DmaControllable<Item> {
    memory_location: *mut Item,
}

impl<Item> DmaControllable<Item> {
    pub(crate) fn new(memory_location: *mut Item) -> Self {
        Self { memory_location }
    }
}

pub struct DmaTransferHandle<'a, T>
where
    T: Copy,
{
    number: usize,
    data: Pin<Box<[T]>>,

    phantom: PhantomData<&'a ()>,
}

impl<'a, T> DmaTransferHandle<'a, T>
where
    T: Copy,
{
    pub(crate) unsafe fn new(number: usize, data: &[T]) -> Self {
        Self {
            number,
            data: Box::into_pin(data.into()),
            phantom: PhantomData,
        }
    }
}

impl<'a, T> Drop for DmaTransferHandle<'a, T>
where
    T: Copy,
{
    fn drop(&mut self) {
        unsafe {
            Dma::new(self.number).disable();
        }
    }
}

const fn dma_source_addr(dma: usize) -> usize {
    0x0400_00b0 + 0x0c * dma
}

const fn dma_dest_addr(dma: usize) -> usize {
    0x0400_00b4 + 0x0c * dma
}

const fn dma_control_addr(dma: usize) -> usize {
    0x0400_00b8 + 0x0c * dma
}

const DMA3_SOURCE_ADDR: MemoryMapped<u32> = unsafe { MemoryMapped::new(dma_source_addr(3)) };
const DMA3_DEST_ADDR: MemoryMapped<u32> = unsafe { MemoryMapped::new(dma_dest_addr(3)) };
const DMA3_CONTROL: MemoryMapped<u32> = unsafe { MemoryMapped::new(dma_control_addr(3)) };

pub(crate) unsafe fn dma_copy16(src: *const u16, dest: *mut u16, count: usize) {
    assert!(count < u16::MAX as usize);

    DMA3_SOURCE_ADDR.set(src as u32);
    DMA3_DEST_ADDR.set(dest as u32);

    DMA3_CONTROL.set(count as u32 | (1 << 31));
}

pub(crate) fn dma3_exclusive<R>(f: impl FnOnce() -> R) -> R {
    const DMA0_CTRL_HI: MemoryMapped<u16> = unsafe { MemoryMapped::new(dma_control_addr(0) + 2) };
    const DMA1_CTRL_HI: MemoryMapped<u16> = unsafe { MemoryMapped::new(dma_control_addr(1) + 2) };
    const DMA2_CTRL_HI: MemoryMapped<u16> = unsafe { MemoryMapped::new(dma_control_addr(2) + 2) };

    crate::interrupt::free(|_| {
        let dma0_ctl = DMA0_CTRL_HI.get();
        let dma1_ctl = DMA1_CTRL_HI.get();
        let dma2_ctl = DMA2_CTRL_HI.get();
        DMA0_CTRL_HI.set(dma0_ctl & !(1 << 15));
        DMA1_CTRL_HI.set(dma1_ctl & !(1 << 15));
        DMA2_CTRL_HI.set(dma2_ctl & !(1 << 15));

        // Executes the body of the function with DMAs and IRQs disabled.
        let ret = f();

        // Continues higher priority DMAs if they were enabled before.
        DMA0_CTRL_HI.set(dma0_ctl);
        DMA1_CTRL_HI.set(dma1_ctl);
        DMA2_CTRL_HI.set(dma2_ctl);

        // returns the return value
        ret
    })
}
