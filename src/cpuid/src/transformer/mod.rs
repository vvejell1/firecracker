// Copyright 2019 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub mod amd;
pub mod common;
pub mod intel;

pub use kvm_bindings::{kvm_cpuid_entry2, CpuId};

use crate::brand_string::{BrandString, Reg as BsReg};
use crate::common::get_vendor_id_from_host;

/// Structure containing the specifications of the VM
pub struct VmSpec {
    /// The vendor id of the CPU
    cpu_vendor_id: [u8; 12],
    /// The desired brand string for the guest.
    brand_string: BrandString,

    /// The index of the current logical cpu in the range [0..cpu_count].
    cpu_index: u8,
    /// The total number of logical cpus.
    cpu_count: u8,

    /// The number of bits needed to enumerate logical CPUs per core.
    cpu_bits: u8,
}

impl VmSpec {
    /// Creates a new instance of VmSpec with the specified parameters
    /// The brand string is deduced from the vendor_id
    pub fn new(cpu_index: u8, cpu_count: u8, smt: bool) -> Result<VmSpec, Error> {
        let cpu_vendor_id = get_vendor_id_from_host()?;

        Ok(VmSpec {
            cpu_vendor_id,
            cpu_index,
            cpu_count,
            cpu_bits: (cpu_count > 1 && smt) as u8,
            brand_string: BrandString::from_vendor_id(&cpu_vendor_id),
        })
    }

    /// Returns an immutable reference to cpu_vendor_id
    pub fn cpu_vendor_id(&self) -> &[u8; 12] {
        &self.cpu_vendor_id
    }

    /// Returns the number of cpus per core
    pub fn cpus_per_core(&self) -> u8 {
        1 << self.cpu_bits
    }
}

/// Errors associated with processing the CPUID leaves.
#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    /// A FamStructWrapper operation has failed
    #[error("A FamStructWrapper operation has failed.")]
    Fam(utils::fam::Error),
    /// A call to an internal helper method failed
    #[error("A call to an internal helper method failed: {0}")]
    InternalError(#[from] super::common::Error),
    /// The operation is not permitted for the current vendor
    #[error("The operation is not permitted for the current vendor.")]
    InvalidVendor,
    /// The maximum number of addressable logical CPUs cannot be stored in an `u8`.
    #[error("The maximum number of addressable logical CPUs cannot be stored in an `u8`.")]
    VcpuCountOverflow,
}

pub type EntryTransformerFn =
    fn(entry: &mut kvm_cpuid_entry2, vm_spec: &VmSpec) -> Result<(), Error>;

/// Generic trait that provides methods for transforming the cpuid
pub trait CpuidTransformer {
    /// Trait main function. It processes the cpuid and makes the desired transformations.
    /// The default logic can be overwritten if needed. For example see `AmdCpuidTransformer`.
    fn process_cpuid(&self, cpuid: &mut CpuId, vm_spec: &VmSpec) -> Result<(), Error> {
        self.process_entries(cpuid, vm_spec)
    }

    /// Iterates through all the cpuid entries and calls the associated transformer for each one.
    fn process_entries(&self, cpuid: &mut CpuId, vm_spec: &VmSpec) -> Result<(), Error> {
        for entry in cpuid.as_mut_slice().iter_mut() {
            let maybe_transformer_fn = self.entry_transformer_fn(entry);

            if let Some(transformer_fn) = maybe_transformer_fn {
                transformer_fn(entry, vm_spec)?;
            }
        }

        Ok(())
    }

    /// Gets the associated transformer for a cpuid entry
    fn entry_transformer_fn(&self, _entry: &mut kvm_cpuid_entry2) -> Option<EntryTransformerFn> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vmspec() {
        let vm_spec = VmSpec::new(0, 1, true).unwrap();
        assert_eq!(vm_spec.cpu_bits, 0);
        assert_eq!(vm_spec.cpus_per_core(), 1);

        let vm_spec = VmSpec::new(0, 1, false).unwrap();
        assert_eq!(vm_spec.cpu_bits, 0);
        assert_eq!(vm_spec.cpus_per_core(), 1);

        let vm_spec = VmSpec::new(0, 2, false).unwrap();
        assert_eq!(vm_spec.cpu_bits, 0);
        assert_eq!(vm_spec.cpus_per_core(), 1);

        let vm_spec = VmSpec::new(0, 2, true).unwrap();
        assert_eq!(vm_spec.cpu_bits, 1);
        assert_eq!(vm_spec.cpus_per_core(), 2);
    }

    const PROCESSED_FN: u32 = 1;
    const EXPECTED_INDEX: u32 = 100;

    fn transform_entry(entry: &mut kvm_cpuid_entry2, _vm_spec: &VmSpec) -> Result<(), Error> {
        entry.index = EXPECTED_INDEX;

        Ok(())
    }

    struct MockCpuidTransformer {}

    impl CpuidTransformer for MockCpuidTransformer {
        fn entry_transformer_fn(&self, entry: &mut kvm_cpuid_entry2) -> Option<EntryTransformerFn> {
            match entry.function {
                PROCESSED_FN => Some(transform_entry),
                _ => None,
            }
        }
    }

    #[test]
    fn test_process_cpuid() {
        let num_entries = 5;

        let mut cpuid = CpuId::new(num_entries).unwrap();
        let vm_spec = VmSpec::new(0, 1, false);
        cpuid.as_mut_slice()[0].function = PROCESSED_FN;
        assert!(MockCpuidTransformer {}
            .process_cpuid(&mut cpuid, &vm_spec.unwrap())
            .is_ok());

        assert!(cpuid.as_mut_slice().len() == num_entries);
        for entry in cpuid.as_mut_slice().iter() {
            match entry.function {
                PROCESSED_FN => {
                    assert_eq!(entry.index, EXPECTED_INDEX);
                }
                _ => {
                    assert_ne!(entry.index, EXPECTED_INDEX);
                }
            }
        }
    }
}
