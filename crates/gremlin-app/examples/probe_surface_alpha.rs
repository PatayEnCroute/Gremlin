//! Diagnostic : modes de composition alpha offerts par la surface graphique.
//!
//! La fenêtre du familier est déclarée transparente, mais c'est la **surface de
//! présentation** qui décide si le canal alpha est honoré ou aplati. `pixels`
//! retient sans le dire `surface_capabilities.alpha_modes[0]` — le premier mode
//! que rapporte le pilote — et n'expose aucun réglage pour en choisir un autre.
//!
//! Cet exemple crée une fenêtre transparente invisible, interroge la surface, et
//! imprime la liste complète des modes dans l'ordre où le pilote les annonce. Si
//! `Opaque` arrive en tête, tout pixel transparent du familier sera composé en
//! noir : c'est le carré noir observé autour du personnage.
//!
//! ```bash
//! cargo run -p gremlin-app --example probe_surface_alpha
//! ```

use pixels::wgpu;
use std::process::ExitCode;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

/// Côté de la fenêtre de sonde, en unités logiques.
const PROBE_SIDE: f64 = 64.0;

#[derive(Default)]
struct Probe {
    done: bool,
    report: Vec<String>,
}

impl ApplicationHandler for Probe {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.done {
            return;
        }
        self.done = true;

        // Mêmes attributs que la fenêtre du familier, mais invisible : la sonde
        // ne doit pas clignoter sur le bureau.
        let attributes = Window::default_attributes()
            .with_title("Sonde alpha Gremlin")
            .with_inner_size(LogicalSize::new(PROBE_SIDE, PROBE_SIDE))
            .with_transparent(true)
            .with_decorations(false)
            .with_visible(false);

        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(e) => {
                self.report
                    .push(format!("échec de création de fenêtre : {e}"));
                event_loop.exit();
                return;
            }
        };

        for (label, backends) in [
            ("Vulkan", wgpu::Backends::VULKAN),
            ("DX12", wgpu::Backends::DX12),
            ("OpenGL", wgpu::Backends::GL),
        ] {
            self.probe_backend(label, backends, &window);
        }

        event_loop.exit();
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
}

impl Probe {
    /// Interroge un backend donne sur la meme fenetre.
    fn probe_backend(&mut self, label: &str, backends: wgpu::Backends, window: &Arc<Window>) {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..Default::default()
        });

        let surface = match instance.create_surface(window.clone()) {
            Ok(surface) => surface,
            Err(e) => {
                self.report
                    .push(format!("{label:<7} : surface indisponible ({e})"));
                return;
            }
        };

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        }));

        let Some(adapter) = adapter else {
            self.report
                .push(format!("{label:<7} : aucun adaptateur compatible"));
            return;
        };

        let info = adapter.get_info();
        let capabilities = surface.get_capabilities(&adapter);
        let honours_alpha = capabilities.alpha_modes.iter().any(|mode| {
            matches!(
                mode,
                wgpu::CompositeAlphaMode::PreMultiplied | wgpu::CompositeAlphaMode::PostMultiplied
            )
        });

        self.report.push(format!(
            "{label:<7} : {:<28} modes = {:?}  -> retenu {:?}  {}",
            info.name,
            capabilities.alpha_modes,
            capabilities.alpha_modes.first(),
            if honours_alpha {
                "ALPHA POSSIBLE"
            } else {
                "aucun mode alpha"
            }
        ));
    }
}

fn main() -> ExitCode {
    let event_loop = match EventLoop::new() {
        Ok(event_loop) => event_loop,
        Err(e) => {
            eprintln!("boucle d'événements indisponible : {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut probe = Probe::default();
    if let Err(e) = event_loop.run_app(&mut probe) {
        eprintln!("échec de la sonde : {e}");
        return ExitCode::FAILURE;
    }

    for line in &probe.report {
        println!("{line}");
    }

    ExitCode::SUCCESS
}
