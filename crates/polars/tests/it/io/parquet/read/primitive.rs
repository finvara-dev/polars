use polars_parquet::parquet::deserialize::{
    HybridRleDecoderIter, HybridRleIter, SliceFilteredIter,
};
use polars_parquet::parquet::encoding::hybrid_rle::Decoder;
use polars_parquet::parquet::encoding::Encoding;
use polars_parquet::parquet::page::{split_buffer, DataPage, EncodedSplitBuffer};
use polars_parquet::parquet::schema::Repetition;
use polars_parquet::parquet::types::NativeType;
use polars_parquet::read::ParquetError;

use super::dictionary::PrimitivePageDict;
use super::utils::{deserialize_optional, native_cast, Casted, NativePageState, OptionalValues};

/// The deserialization state of a `DataPage` of `Primitive` parquet primitive type
#[derive(Debug)]
pub enum FilteredPageState<'a, T>
where
    T: NativeType,
{
    /// A page of optional values
    Optional(SliceFilteredIter<OptionalValues<T, HybridRleDecoderIter<'a>, Casted<'a, T>>>),
    /// A page of required values
    Required(SliceFilteredIter<Casted<'a, T>>),
}

/// The deserialization state of a `DataPage` of `Primitive` parquet primitive type
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum PageState<'a, T>
where
    T: NativeType,
{
    Nominal(NativePageState<'a, T, &'a PrimitivePageDict<T>>),
    Filtered(FilteredPageState<'a, T>),
}

impl<'a, T: NativeType> PageState<'a, T> {
    /// Tries to create [`NativePageState`]
    /// # Error
    /// Errors iff the page is not a `NativePageState`
    pub fn try_new(
        page: &'a DataPage,
        dict: Option<&'a PrimitivePageDict<T>>,
    ) -> Result<Self, ParquetError> {
        if let Some(selected_rows) = page.selected_rows() {
            let is_optional =
                page.descriptor.primitive_type.field_info.repetition == Repetition::Optional;

            match (page.encoding(), dict, is_optional) {
                (Encoding::Plain, _, true) => {
                    let EncodedSplitBuffer {
                        rep: _,
                        def: def_levels,
                        values: _,
                    } = split_buffer(page)?;

                    let validity = HybridRleDecoderIter::new(HybridRleIter::new(
                        Decoder::new(def_levels, 1),
                        page.num_values(),
                    ));
                    let values = native_cast(page)?;

                    // validity and values interleaved.
                    let values = OptionalValues::new(validity, values);

                    let values =
                        SliceFilteredIter::new(values, selected_rows.iter().copied().collect());

                    Ok(Self::Filtered(FilteredPageState::Optional(values)))
                },
                (Encoding::Plain, _, false) => {
                    let values = SliceFilteredIter::new(
                        native_cast(page)?,
                        selected_rows.iter().copied().collect(),
                    );
                    Ok(Self::Filtered(FilteredPageState::Required(values)))
                },
                _ => Err(ParquetError::FeatureNotSupported(format!(
                    "Viewing page for encoding {:?} for native type {}",
                    page.encoding(),
                    std::any::type_name::<T>()
                ))),
            }
        } else {
            NativePageState::try_new(page, dict).map(Self::Nominal)
        }
    }
}

pub fn page_to_vec<T: NativeType>(
    page: &DataPage,
    dict: Option<&PrimitivePageDict<T>>,
) -> Result<Vec<Option<T>>, ParquetError> {
    assert_eq!(page.descriptor.max_rep_level, 0);
    let state = PageState::<T>::try_new(page, dict)?;

    match state {
        PageState::Nominal(state) => match state {
            NativePageState::Optional(validity, mut values) => {
                deserialize_optional(validity, values.by_ref().map(Ok))
            },
            NativePageState::Required(values) => Ok(values.map(Some).collect()),
            NativePageState::RequiredDictionary(dict) => dict
                .indexes
                .iter()
                .map(|x| dict.dict.value(x as usize).copied().map(Some))
                .collect(),
            NativePageState::OptionalDictionary(validity, dict) => {
                let values = dict
                    .indexes
                    .iter()
                    .map(|x| dict.dict.value(x as usize).copied());
                deserialize_optional(validity, values)
            },
        },
        PageState::Filtered(state) => match state {
            FilteredPageState::Optional(values) => Ok(values.collect()),
            FilteredPageState::Required(values) => Ok(values.map(Some).collect()),
        },
    }
}
