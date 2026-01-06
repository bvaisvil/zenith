/**
 * Copyright 2019-2022, Benjamin Vaisvil and the zenith contributors
 */
#[macro_export]
macro_rules! float_to_byte_string {
    ($x:expr, $unit:expr) => {
        match Byte::from_f64_with_unit($x, $unit) {
            Some(b) => format!("{:.2}", b.get_appropriate_unit(byte_unit::UnitType::Binary))
                .replace(" ", "")
                .replace("iB", ""),
            None => String::from("Err"),
        }
    };
}
