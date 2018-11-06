#[inline] pub fn decode(line: &[u8], bit_offset: u8, length: u8) -> u32 {
    let start_mask = !0 >> (bit_offset % 8);
    let stop_mask = !(!0 << ((bit_offset+length) % 8));
    
    let start_byte = (bit_offset /8) as usize;
    let stop_byte = ((bit_offset+length) /8) as usize;
  
    //decode first bit (never needs shifting (lowest part is used))
    let mut decoded: u32 = 0 | (line[start_byte] & start_mask) as u32;
    let mut bits_read = 8-(bit_offset % 8);
    //if we have more bits 
    //if length-8 > 8-bit_offset%8 {
        //decode middle bits, no masking needed
        for (i, byte) in line[start_byte+1..stop_byte].iter().enumerate(){
            decoded |= (*byte as u32) << (8-(bit_offset % 8) + (i as u8) *8) ;
            bits_read+= 8;
        }
    //}
    decoded |= ((line[stop_byte] & stop_mask) as u32) << bits_read;
    
    decoded
}

#[inline] pub fn encode(to_encode: u32, line: &mut [u8], bit_offset: u8, length: u8) {

    let start_mask = !0 >> (bit_offset % 8);
    
    let start_byte = (bit_offset /8) as usize;
    let stop_byte = ((bit_offset+length) /8) as usize;
    
    //decode first bit (never needs shifting (lowest part is used))
    line[start_byte] |= (to_encode as u8) & start_mask;
    let mut bits_written = 8-(bit_offset % 8);
    
    //if we have more bits 
    //if length-8 > 8-bit_offset%8 {
        //decode middle bits, no masking needed
    for byte in line[start_byte+1..stop_byte].iter_mut(){
        *byte |= (to_encode >> bits_written) as u8;
        bits_written += 8;
    }
    line[stop_byte] |= (to_encode >> bits_written) as u8;
}

#[cfg(test)]
	#[test]
	fn encode_and_decode(){
		for length in 8..32 {
			for offset in 0..16 {
				for _power in 0..length as u16 *10 { 
					let power = _power as f32*0.1;
					let mut array = vec!(0;8);
					let test_numb = 2f32.powf(power) as u32;
					encode(test_numb, array.as_mut_slice(), offset, length);
					let decoded_test_numb = decode(array.as_slice(), offset, length); 
					assert_eq!(test_numb, decoded_test_numb);
				}
			}
		}
	}
