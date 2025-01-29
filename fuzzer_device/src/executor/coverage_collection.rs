use alloc::boxed::Box;
use alloc::format;
use alloc::string::ToString;
use core::pin::Pin;
use coverage::harness::coverage_harness::hookable_address_iterator::HookableAddressIterator;
use coverage::harness::coverage_harness::modification_engine::ModificationEngineSettings;
use coverage::harness::coverage_harness::{
    CoverageError, CoverageExecutionResult, CoverageHarness,
};
use coverage::harness::iteration_harness::{IterationHarness};
use coverage::interface::safe::ComInterface;
use coverage::interface_definition;
use custom_processing_unit::CustomProcessingUnit;
use data_types::addresses::UCInstructionAddress;

// safety: Self referential struct, drop in reverse order
// should be safe since no references are released to the outside
// references inside are towards Pin<Box>>
pub struct CoverageCollector {
    custom_processing_unit: Option<Pin<Box<CustomProcessingUnit>>>,
    com_interface: Option<Pin<Box<ComInterface<'static>>>>,
    coverage_harness: Option<CoverageHarness<'static, 'static, 'static>>,

    hooks: usize,
    modification_engine_settings: ModificationEngineSettings,
}

impl CoverageCollector {
    pub fn initialize() -> Result<Self, custom_processing_unit::Error> {
        let mut cpu = CustomProcessingUnit::new()?;

        cpu.init()?;
        cpu.zero_hooks()?;

        let cpu = Box::pin(cpu);

        let interface = match ComInterface::new(&interface_definition::COM_INTERFACE_DESCRIPTION) {
            Ok(interface) => interface,
            Err(e) => {
                return Err(custom_processing_unit::Error::Other(format!(
                    "Failed to create COM interface: {:?}",
                    e
                )));
            }
        };
        let mut interface = Box::pin(interface);
        let hooks = {
            let max_hooks = interface.description().max_number_of_hooks;

            let device_max_hooks = match cpu.current_glm_version {
                custom_processing_unit::GLM_OLD => 31,
                custom_processing_unit::GLM_NEW => 32,
                _ => 0,
            };

            max_hooks.min(device_max_hooks)
        };

        if hooks == 0 {
            return Err(custom_processing_unit::Error::Other(
                "No hooks available".to_string(),
            ));
        }

        let interface_static: &'static mut ComInterface =
            unsafe { core::mem::transmute(&mut interface) };
        let cpu_static: &'static CustomProcessingUnit = unsafe { core::mem::transmute(&cpu) };

        let harness = match CoverageHarness::new(interface_static, cpu_static) {
            Ok(harness) => harness,
            Err(e) => {
                return Err(custom_processing_unit::Error::Other(format!(
                    "Failed to initiate harness {:?}",
                    e
                )));
            }
        };

        Ok(Self {
            custom_processing_unit: Some(cpu),
            com_interface: Some(interface),
            coverage_harness: Some(harness),
            modification_engine_settings: ModificationEngineSettings::default(),
            hooks,
        })
    }

    pub fn get_iteration_harness(&self) -> IterationHarness {
        let hookable_addresses = HookableAddressIterator::construct(
            self.custom_processing_unit.as_ref().unwrap().rom(), // safe, since rom() -> 'static, no side effects
            &self.modification_engine_settings,
            self.hooks,
        );

        IterationHarness::new(hookable_addresses)
    }

    pub fn execute_coverage_collection<FuncResult, F: FnOnce() -> FuncResult>(
        &mut self,
        hooks: &[UCInstructionAddress],
        func: F,
    ) -> Result<CoverageExecutionResult<FuncResult>, CoverageError> {
        match &mut self.coverage_harness {
            Some(harness) => harness.execute(hooks, func),
            None => unreachable!("since coverage_harness is always Some, until dropped"),
        }
    }
}

impl Drop for CoverageCollector {
    fn drop(&mut self) {
        // drop in reverse order
        drop(self.coverage_harness.take());
        drop(self.com_interface.take());
        drop(self.custom_processing_unit.take())
    }
}
