use coverage::harness::coverage_harness::CoverageHarness;
use coverage::interface::safe::ComInterface;
use custom_processing_unit::CustomProcessingUnit;

pub struct CustomProcessingUnitData {
    custom_processing_unit: CustomProcessingUnit,
    com_interface: ComInterface<'static>,
    coverage_harness: CoverageHarness<'static, 'static, 'static>,
}

