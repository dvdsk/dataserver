extern crate minimal_timeseries;
extern crate serde_yaml;

use std::fs;
use std::io;
use std::path::Path;

use super::{Field, FieldId, MetaData};
use super::super::{rand, rand::{FromEntropy, Rng}};

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct FieldLength {
	name: String,
	min_value: f32,
	max_value: f32,
	numb_of_bits: u8, //bits (max 32 bit variables)
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct FieldSigDigits {
	name: String,
	min_value: f32,
	max_value: f32,
	number_of_digits: u32,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct FieldManual {
	name: String,
    length: u8,
    decode_scale: f32,
    decode_add: f32,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum FieldSpec{
	BitLength(FieldLength),
	SigDigits(FieldSigDigits),
	Manual(FieldManual),
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct MetaDataSpec {
	pub name: String,
	pub description: String,
	pub fields: Vec<FieldSpec>,//must be sorted lowest id to highest
}

impl Into<MetaData> for MetaDataSpec {
    fn into(mut self) -> MetaData {
        
        let mut fields = Vec::new();
        let mut start_bit = 0;
        //convert every field enum in the fields vector into a field
        for (id, field) in self.fields.drain(..).enumerate() {
            let (decode_scale, length, name, decode_add) = match field {
				FieldSpec::BitLength(field) => {
					let max_storable = 2_u32.pow(field.numb_of_bits as u32) as f32;
					let decode_scale = ((field.max_value - field.min_value)/max_storable).ceil() ;
					
					let length = field.numb_of_bits;
					let name = field.name;
					let decode_add = field.min_value;
					(decode_scale, length, name, decode_add)
				}
				FieldSpec::SigDigits(field) => {
					let normalised_range = (field.max_value - field.min_value)/field.max_value;
					let needed_range = 10_u32.pow(field.number_of_digits) as f32 *normalised_range;
					let length = needed_range.log2().ceil() as u8;
					let decode_scale = 10_f32.powf( -1.0*(field.number_of_digits as f32 -field.max_value.log10()) );
					let name = field.name;
					let decode_add = field.min_value;
					(decode_scale, length, name, decode_add)
				}
				FieldSpec::Manual(field) => {
					let length = field.length;
					let decode_scale = field.decode_scale;
					let name = field.name;
					let decode_add = field.decode_add;
					(decode_scale, length, name, decode_add)
				}
			};
            fields.push(Field::<f32> {
                id: id as FieldId,
                name: name,
                offset: start_bit,
                length: length,
                decode_scale: decode_scale,
                decode_add: decode_add,
            });
            start_bit += length;
        }
        //set the security key to a random value
        let mut rng = rand::StdRng::from_entropy();
        MetaData {
			name: self.name,
			description: self.description,
			key: rng.gen(),
			fields: fields,//must be sorted lowest id to highest
		}
    }
}

pub fn write_template() -> io::Result<()> {
	let template_field_1 = FieldSpec::BitLength( FieldLength {
		name: String::from("template field name1"),
		min_value: 0.01f32,
		max_value: 10f32,
		numb_of_bits: 10u8, //bits (max 32 bit variables)
	});
	let template_field_2 = FieldSpec::SigDigits( FieldSigDigits {
		name: String::from("template field name2"),
		min_value: 0f32,
		max_value: 100f32,
		number_of_digits: 8u32,
	});
	let template_field_3 = FieldSpec::Manual( FieldManual {
		name: String::from("template field name3"),
		length: 10,
		decode_scale: 0.1,
		decode_add: -40f32,
	});
	let metadata = MetaDataSpec {
		name: String::from("template dataset name"),
		description: String::from("This is a template it is not to be used for storing data, please copy this file and edit it. Then use the new file for creating new datasets"),
		fields: vec!(template_field_1, template_field_2, template_field_3),
	};
	
	if !Path::new("specs").exists() {fs::create_dir("specs")? }
	match fs::File::create("specs/template.yaml"){
		Ok(f) => {
			if serde_yaml::to_writer(f, &metadata).is_err() {
				Err(io::Error::new(io::ErrorKind::InvalidData, "could not parse specification"))
			} else { Ok(()) }
		},
		Err(error) => Err(error),
	}
}
