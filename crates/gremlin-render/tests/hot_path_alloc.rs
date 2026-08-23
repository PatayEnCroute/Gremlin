//! Régression d'allocation du chemin chaud visuel.

use gremlin_render::{
    register_default_procedural_accessories, AccessoryCatalog, AccessoryCategory, BubbleRect,
    LayerCompositor, ParticleEngine, ParticlePreset, PixelBuffer, SpeechBubbleRenderer,
    SpeechBubbleView, SpriteAtlas, TransitionRenderer, WardrobeEquipment,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;
use std::time::Duration;

thread_local! {
    static TRACK_ALLOCATIONS: Cell<bool> = const { Cell::new(false) };
    static ALLOCATION_COUNT: Cell<usize> = const { Cell::new(0) };
}

struct ThreadCountingAllocator;

#[allow(unsafe_code)]
unsafe impl GlobalAlloc for ThreadCountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        // SAFETY: délégation directe au gestionnaire système avec le même layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        // SAFETY: délégation directe au gestionnaire système avec le même layout.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: le pointeur provient du gestionnaire système avec ce layout.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count_allocation();
        // SAFETY: le pointeur et le layout proviennent du gestionnaire système.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: ThreadCountingAllocator = ThreadCountingAllocator;

fn count_allocation() {
    TRACK_ALLOCATIONS.with(|tracking| {
        if tracking.get() {
            ALLOCATION_COUNT.with(|count| count.set(count.get().saturating_add(1)));
        }
    });
}

fn measured_allocations(action: impl FnOnce()) -> usize {
    // Initialise les cellules TLS avant d'activer la mesure.
    TRACK_ALLOCATIONS.with(|tracking| tracking.set(false));
    ALLOCATION_COUNT.with(|count| count.set(0));
    TRACK_ALLOCATIONS.with(|tracking| tracking.set(true));
    action();
    TRACK_ALLOCATIONS.with(|tracking| tracking.set(false));
    ALLOCATION_COUNT.with(Cell::get)
}

#[test]
fn hot_path_visuel_nalloue_pas_apres_prechauffage() {
    let mut atlas = SpriteAtlas::new();
    let mut catalog = AccessoryCatalog::new();
    register_default_procedural_accessories(&mut atlas, &mut catalog);
    atlas.load_default_procedural_sprites();

    let mut equipment = WardrobeEquipment::new();
    equipment.equip(AccessoryCategory::Hat, "wizard_hat");
    equipment.equip(AccessoryCategory::Held, "coffee_mug");

    let mut output = PixelBuffer::new(64, 64);
    let mut outgoing = PixelBuffer::new(64, 64);
    let mut incoming = PixelBuffer::new(64, 64);
    let mut particles = ParticleEngine::with_seed(42);
    particles.emit(ParticlePreset::SparkBurst, (32, 30));
    outgoing.clear(20, 40, 60, 255);
    incoming.clear(80, 100, 120, 255);

    let bubble = SpeechBubbleView {
        text: "Bon commit",
        opacity: 220,
        bounds: BubbleRect::companion_default(),
        target_anchor: (32, 18),
    };

    // Préchauffage des chemins et des éventuelles initialisations paresseuses.
    LayerCompositor::compose_layered_pet_animated(
        &mut output,
        &equipment,
        &atlas,
        None,
        &catalog,
        "idle_0",
        "happy",
        Duration::from_millis(250),
    );
    particles.update(Duration::from_millis(16));
    particles.render(&mut output);
    SpeechBubbleRenderer::render(&mut output, bubble);
    TransitionRenderer::blend(&outgoing, &incoming, &mut output, 128, -1);

    let allocations = measured_allocations(|| {
        output.clear(0, 0, 0, 0);
        LayerCompositor::compose_layered_pet_animated(
            black_box(&mut output),
            black_box(&equipment),
            black_box(&atlas),
            None,
            black_box(&catalog),
            black_box("idle_0"),
            black_box("happy"),
            black_box(Duration::from_millis(450)),
        );
        black_box(particles.update(Duration::from_millis(16)));
        particles.render(black_box(&mut output));
        SpeechBubbleRenderer::render(black_box(&mut output), black_box(bubble));
        black_box(TransitionRenderer::blend(
            black_box(&outgoing),
            black_box(&incoming),
            black_box(&mut output),
            black_box(192),
            black_box(0),
        ));
    });

    assert_eq!(allocations, 0, "allocations détectées : {allocations}");
}
