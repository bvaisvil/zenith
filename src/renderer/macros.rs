/**
 * Copyright 2019-2022, Benjamin Vaisvil and the zenith contributors
 */
#[macro_export]
macro_rules! float_to_byte_string {
    ($x:expr, $unit:expr) => {
        match Byte::from_unit($x, $unit) {
            Ok(b) => b.get_appropriate_unit(false).to_string().replace(" ", ""),
            Err(_) => String::from("Err"),
        }
    };
}
