//! Local preflight for explicitly selected TKGM architectural model workflows.
//!
//! This is a bounded subset of the public production guide, not TKGM acceptance.
//! Geometry validation, official XSD validation, identifiers checked against MAKS /
//! TAKBIS, naming rules, all code lists and the online rule engine remain separate.
//! Source: https://cbs.tkgm.gov.tr/surdurulebilirlik/ (retrieved 2026-10-01).
//! Tender source: https://cbs.tkgm.gov.tr/3d/html/ (retrieved 2026-10-01).

use crate::{
    CityGmlDocument, ValidationOptions,
    native::*,
    validate::{elements, validate},
};
use cadkit_core::{Error, Limits, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Workflow selection is required; a recipient name alone does not identify its rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    /// Architectural city-model tender common rules from production guide v2.50.
    /// Tender-year-specific MAKS and sustainability extensions remain unchecked.
    CityModelTender,
    /// Digital Building condominium/easement registration guide 3.0.3.
    DigitalBuildingRegistration,
}

impl std::str::FromStr for Profile {
    type Err = Error;

    fn from_str(name: &str) -> Result<Self> {
        match name {
            "city-model-tender" => Ok(Self::CityModelTender),
            "digital-building-registration" => Ok(Self::DigitalBuildingRegistration),
            _ => Err(Error::invalid(
                0,
                "TKGM profile must be city-model-tender or digital-building-registration",
            )),
        }
    }
}

/// A local profile finding. No source names, IDs, attribute values or coordinates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileIssue {
    /// Stable code, for example `tkgm.attribute_type`.
    pub code: String,
    /// `error` for a definite local-rule mismatch, `review` for conditional cases.
    /// Neither severity establishes the receiving authority's decision.
    pub severity: String,
    /// Zero-based index in the bounded native element traversal.
    pub element: usize,
    /// Public schema field name, when relevant.
    pub field: Option<String>,
}

/// Results of the documented local subset. Empty issues never imply online acceptance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileReport {
    /// Version of the public guide used by these checks, not the OGC CityGML version.
    pub profile: String,
    /// Explicitly selected workflow; never inferred from source values.
    pub workflow: String,
    /// Always false: this API cannot execute the official TKGM rule engine.
    pub acceptance_verified: bool,
    /// Building objects inspected.
    pub buildings: u64,
    /// Groups classified as storeys and inspected.
    pub storeys: u64,
    /// Generic objects classified as independent sections and inspected.
    pub independent_sections: u64,
    /// Missing or inconsistent fields in the checked subset.
    pub issues: Vec<ProfileIssue>,
    /// True when findings exceeded min(max_objects, 10,000); no clean result is implied.
    pub truncated: bool,
    /// Known checks deliberately outside this local subset, as stable names.
    pub unchecked_rules: Vec<String>,
    #[serde(skip)]
    issue_limit: usize,
}

const BUILDING_FIELDS: &[(&str, &str)] = &[
    ("constructionID", "stringAttribute"),
    ("maksIdentityNumber", "intAttribute"),
    ("maksIntegrationState", "intAttribute"),
    ("takbisPropertyIdentityNumber", "intAttribute"),
    ("blockNumber", "stringAttribute"),
    ("parcelNumber", "stringAttribute"),
    ("tenderRegistrationNumber", "stringAttribute"),
    ("buildingType", "intAttribute"),
    ("geometrySuitability", "intAttribute"),
    ("architecturalPlanID", "stringAttribute"),
    ("takbisNeighbourhoodReference", "intAttribute"),
    ("totalIndependentSectionCount", "intAttribute"),
    ("elevatorCount", "intAttribute"),
    ("roofProjectionArea", "doubleAttribute"),
    ("architecturalProjectConfirmationDate", "dateAttribute"),
];
const STOREY_FIELDS: &[(&str, &str)] = &[
    ("storeyNumber", "stringAttribute"),
    ("storeyUsage", "intAttribute"),
    ("independentSectionCount", "intAttribute"),
];
const UNIT_FIELDS: &[(&str, &str)] = &[
    ("geometrySuitability", "intAttribute"),
    ("integrationState", "intAttribute"),
    ("independentSectionNumber", "stringAttribute"),
    ("blockName", "stringAttribute"),
    ("entrance", "stringAttribute"),
    ("takbisPropertyIdentityNumber", "intAttribute"),
    ("maksIdentityNumber", "intAttribute"),
    ("maksNumaratajIdentityNumber", "intAttribute"),
    ("maksIntegrationState", "intAttribute"),
    ("independentSectionCardinalDirection", "intAttribute"),
    ("independentSectionUsage", "intAttribute"),
    ("partCount", "intAttribute"),
    ("independentSectionPlanNetArea", "doubleAttribute"),
    ("independentSectionPlanGrossArea", "doubleAttribute"),
    ("independentSectionCalculatedArea", "doubleAttribute"),
];

const REGISTRATION_UNIT_FIELDS: &[(&str, &str)] = &[
    ("registeredQuality", "stringAttribute"),
    ("registeredInstallation", "stringAttribute"),
    ("registeredFloorDefinition", "stringAttribute"),
    // The registration table and example disagree; never coerce source values.
    ("propertyLot", ""),
];

// The tender guide's MAKS table has different presence rules before 2019,
// in 2019-2020 and after 2020. Check supplied types, but leave these conditions
// to a contract-specific validator rather than applying registration defaults.
const TENDER_CONDITIONAL_FIELDS: &[&str] = &[
    "maksIdentityNumber",
    "maksNumaratajIdentityNumber",
    "maksIntegrationState",
];

impl ProfileReport {
    fn issue(&mut self, code: &str, element: usize, field: Option<&str>) {
        self.finding(code, "error", element, field);
    }
    fn finding(&mut self, code: &str, severity: &str, element: usize, field: Option<&str>) {
        if self.issues.len() >= self.issue_limit {
            self.truncated = true;
            return;
        }
        self.issues.push(ProfileIssue {
            code: code.into(),
            severity: severity.into(),
            element,
            field: field.map(str::to_owned),
        });
    }
    fn fields(&mut self, e: &Element, index: usize, fields: &[(&str, &str)], conditional: &[&str]) {
        for &(field, kind) in fields {
            let mut count = 0;
            for a in e
                .elements()
                .filter(|a| a.name.namespace == GENERICS && a.attribute("", "name") == Some(field))
            {
                count += 1;
                if !kind.is_empty() && a.name.local() != kind {
                    self.issue("tkgm.attribute_type", index, Some(field));
                }
            }
            if count == 0 && !conditional.contains(&field) {
                self.issue("tkgm.missing_attribute", index, Some(field));
            }
            if count > 1
                && !matches!(field, "storeyUsage" | "independentSectionCardinalDirection")
                && !(e.name.is(BUILDING, "Building") && field == "geometrySuitability")
            {
                self.issue("tkgm.duplicate_attribute", index, Some(field));
            }
        }
    }
    fn required(&mut self, e: &Element, index: usize, ns: &str, field: &str) {
        if !e.elements().any(|c| c.name.is(ns, field)) {
            self.issue("tkgm.missing_property", index, Some(field));
        }
    }
}

fn class(e: &Element, namespace: &str) -> Option<String> {
    e.elements()
        .find(|n| n.name.is(namespace, "class"))
        .map(Element::text)
}

/// Checks required field presence/types, core geometry-property presence, storey
/// parent/member references and explicit TUREF 3D envelope for the selected workflow.
/// Fixed unit codes apply only to Digital Building registration. The tender subset
/// does not check conditional MAKS presence or sustainability extensions by year.
/// Uses the caller's resource limits and preserves all input values unchanged.
/// Run `validate`, official XSD and TKGM validation separately for their own checks.
pub fn preflight(
    doc: &CityGmlDocument,
    profile: Profile,
    limits: &Limits,
) -> Result<ProfileReport> {
    validate(
        doc,
        &ValidationOptions {
            limits: *limits,
            geometry: false,
            ..Default::default()
        },
    )?;
    let nodes = elements(doc, limits)?;
    let ids: HashMap<_, _> = nodes
        .iter()
        .filter_map(|(e, _)| e.attribute(GML, "id").map(|id| (id, *e)))
        .collect();
    let registration = profile == Profile::DigitalBuildingRegistration;
    let conditional = if registration {
        &[][..]
    } else {
        TENDER_CONDITIONAL_FIELDS
    };
    let mut report = ProfileReport {
        profile: if registration {
            "TKGMCityGML 3.0.3 (local subset)"
        } else {
            "TKGM production guide v2.50 (architectural common subset)"
        }
        .into(),
        workflow: if registration {
            "digital_building_registration"
        } else {
            "city_model_tender_architectural"
        }
        .into(),
        acceptance_verified: false,
        buildings: 0,
        storeys: 0,
        independent_sections: 0,
        issues: vec![],
        truncated: false,
        unchecked_rules: [
            "all_code_lists",
            "naming_rules",
            "geometry_property_contents",
            "room_opening_installation_rules",
            "architectural_group_membership",
            "external_identity_values",
            "geometric_topology",
            "online_validation",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        issue_limit: limits.max_objects.min(10_000) as usize,
    };
    report.unchecked_rules.extend(
        if registration {
            [
                "property_lot_type_ambiguity",
                "building_height_type_ambiguity",
            ]
        } else {
            [
                "maks_rules_by_tender_year",
                "sustainability_extensions_by_contract",
            ]
        }
        .into_iter()
        .map(str::to_owned),
    );
    let root_envelope = doc
        .root
        .elements()
        .any(|e| e.name.is(GML, "boundedBy") && e.elements().any(|c| c.name.is(GML, "Envelope")));
    for (index, &(e, _)) in nodes.iter().enumerate() {
        if e.name.is(GML, "Envelope") {
            if e.attribute("", "srsDimension") != Some("3") {
                report.issue("tkgm.envelope_dimension", index, Some("srsDimension"));
            }
            let srs = e.attribute("", "srsName").unwrap_or("");
            let code = srs
                .strip_prefix("EPSG:")
                .or_else(|| srs.strip_prefix("urn:ogc:def:crs:EPSG::"))
                .or_else(|| srs.strip_prefix("http://www.opengis.net/def/crs/EPSG/0/"));
            if !matches!(
                code,
                Some("5253" | "5254" | "5255" | "5256" | "5257" | "5258" | "5259")
            ) {
                report.issue("tkgm.crs", index, Some("srsName"));
            }
        }
        let building = e.name.is(BUILDING, "Building");
        let storey =
            e.name.is(GROUPS, "CityObjectGroup") && class(e, GROUPS).as_deref() == Some("Kat");
        let unit = e.name.is(GENERICS, "GenericCityObject")
            && class(e, GENERICS).as_deref() == Some("BagimsizBolum");
        if (e.name.is(GROUPS, "CityObjectGroup") && class(e, GROUPS).is_none())
            || (e.name.is(GENERICS, "GenericCityObject") && class(e, GENERICS).is_none())
        {
            report.issue("tkgm.missing_class", index, Some("class"));
        }
        if building || storey || unit {
            report.required(e, index, GML, "name");
            if e.attribute(GML, "id").is_none() {
                report.issue("tkgm.missing_id", index, Some("gml:id"));
            }
        }
        if building {
            report.buildings += 1;
            report.fields(e, index, BUILDING_FIELDS, conditional);
            // Only the registration guide has conflicting height types. The tender
            // architectural chapter explicitly specifies doubleAttribute.
            report.fields(
                e,
                index,
                &[(
                    "buildingHeight",
                    if registration { "" } else { "doubleAttribute" },
                )],
                &[],
            );
            if class(e, BUILDING).as_deref() != Some("MimariBina") {
                report.issue("tkgm.building_class", index, Some("class"));
            }
            for field in [
                "storeysAboveGround",
                "storeysBelowGround",
                "lod0FootPrint",
                "lod0RoofEdge",
                "lod1Solid",
                "lod2MultiSurface",
                "lod2TerrainIntersection",
                "boundedBy",
            ] {
                report.required(e, index, BUILDING, field);
            }
        }
        if storey {
            report.storeys += 1;
            report.fields(e, index, STOREY_FIELDS, &[]);
            for field in ["parent", "geometry"] {
                report.required(e, index, GROUPS, field);
            }
            if !e.elements().any(|c| c.name.is(GROUPS, "groupMember")) {
                // Common areas are Rooms with their own storeyObjectReference.
                // An explicitly empty unit count does not prove broken ownership;
                // require review instead of inventing membership from geometry.
                let zero_units = e.elements().any(|a| {
                    a.name.is(GENERICS, "intAttribute")
                        && a.attribute("", "name") == Some("independentSectionCount")
                        && a.elements().any(|v| {
                            v.name.is(GENERICS, "value") && v.text().trim().parse::<i64>() == Ok(0)
                        })
                });
                if zero_units {
                    report.finding(
                        "tkgm.empty_storey_membership",
                        "review",
                        index,
                        Some("groupMember"),
                    );
                } else {
                    report.issue("tkgm.missing_property", index, Some("groupMember"));
                }
            }
            for parent in e.elements().filter(|p| p.name.is(GROUPS, "parent")) {
                let target = parent
                    .attribute(XLINK, "href")
                    .and_then(|r| r.strip_prefix('#'))
                    .and_then(|id| ids.get(id));
                if !target.is_some_and(|t| t.name.is(BUILDING, "Building")) {
                    report.issue("tkgm.storey_parent", index, Some("parent"));
                }
            }
            for member in e.elements().filter(|p| p.name.is(GROUPS, "groupMember")) {
                let target = member
                    .attribute(XLINK, "href")
                    .and_then(|r| r.strip_prefix('#'))
                    .and_then(|id| ids.get(id));
                if !target.is_some_and(|t| {
                    (t.name.is(GENERICS, "GenericCityObject")
                        && class(t, GENERICS).as_deref() == Some("BagimsizBolum"))
                        || (t.name.is(BUILDING, "Room")
                            && class(t, BUILDING).as_deref() == Some("OrtakAlan"))
                }) {
                    report.issue("tkgm.storey_member", index, Some("groupMember"));
                }
            }
        }
        if unit {
            report.independent_sections += 1;
            report.fields(e, index, UNIT_FIELDS, conditional);
            report.required(e, index, GENERICS, "lod2Geometry");
        }
        if unit && registration {
            report.fields(e, index, REGISTRATION_UNIT_FIELDS, &[]);
            for (field, expected) in [
                ("geometrySuitability", 1006),
                ("integrationState", 1004),
                ("maksIntegrationState", 1003),
                ("takbisPropertyIdentityNumber", -1),
            ] {
                for a in e.elements().filter(|a| {
                    a.name.is(GENERICS, "intAttribute") && a.attribute("", "name") == Some(field)
                }) {
                    let value = a
                        .elements()
                        .find(|v| v.name.is(GENERICS, "value"))
                        .map(Element::text);
                    if value.as_deref().and_then(|v| v.trim().parse::<i64>().ok()) != Some(expected)
                    {
                        report.issue("tkgm.fixed_value", index, Some(field));
                    }
                }
            }
        }
        if e.name.is(GENERICS, "stringAttribute")
            && e.attribute("", "name") == Some("cadkit.geometry_status")
        {
            report.issue("tkgm.metadata_only_geometry", index, None);
        }
    }
    if report.buildings == 0 {
        report.issue("tkgm.missing_building", 0, None);
    }
    if !root_envelope {
        report.issue("tkgm.missing_envelope", 0, None);
    }
    Ok(report)
}
