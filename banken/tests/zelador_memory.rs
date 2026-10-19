use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Arc;
use std::sync::atomic::{AtomicIsize, Ordering};

use banken::armazem::Estoque;
use banken::fixture::FixtureClusterEnv;
use izumi_armazem::{ArmazemConfig, PolicyEngine};

struct Counting;

static LIVE: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            LIVE.fetch_add(layout.size().cast_signed(), Ordering::Relaxed);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size().cast_signed(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            LIVE.fetch_add(
                new_size.cast_signed() - layout.size().cast_signed(),
                Ordering::Relaxed,
            );
        }
        p
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

#[test]
fn an_idle_mcp_estoque_does_not_grow_per_zelador_tick() {
    let cfg = ArmazemConfig::default();
    assert_eq!(cfg.policy, PolicyEngine::Vigy, "the served default is the vigy policy");
    let estoque = Estoque::over_env(Arc::new(FixtureClusterEnv::new()), None, &cfg)
        .expect("the default configuration is valid");
    let tick = || {
        estoque.rows.tick().expect("rows zelador decides");
        estoque.logs.tick().expect("logs zelador decides");
    };
    for _ in 0..20 {
        tick();
    }
    let before = LIVE.load(Ordering::Relaxed);
    let seconds = 300;
    for _ in 0..seconds {
        tick();
    }
    let retained = LIVE.load(Ordering::Relaxed) - before;
    assert!(
        retained < 128 * 1024,
        "{seconds} simulated seconds of an idle `banken mcp` retained {retained} bytes; \
         the 0.1.45 process grew to 16 GB this way in under 8 hours"
    );
}
