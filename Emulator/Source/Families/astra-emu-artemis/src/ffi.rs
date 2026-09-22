use crate::provider::ArtemisProvider;
use abi_stable::{
    prefix_type::PrefixTypeTrait, sabi_types::Constructor, type_level::downcasting::TD_Opaque,
};
use astra_emu_family_api::{
    AstraFamilyModule, AstraFamilyModuleRef, FamilyModuleBox, ProviderModule,
};

extern "C" fn construct_module() -> FamilyModuleBox {
    astra_emu_family_api::FamilyModule_TO::from_value(
        ProviderModule::<ArtemisProvider>::default(),
        TD_Opaque,
    )
}

#[abi_stable::export_root_module]
pub fn astra_artemis_family_root_module() -> AstraFamilyModuleRef {
    AstraFamilyModule {
        service: Constructor(construct_module),
    }
    .leak_into_prefix()
}
