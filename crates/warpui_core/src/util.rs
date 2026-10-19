pub fn post_inc(value: &mut usize) -> usize {
    let prev = *value;
    *value += 1;
    prev
}

pub fn parse_u32(input: &[u8]) -> Option<u32> {
    if input.is_empty() {
        return None;
    }
    let mut num: u32 = 0;
    for c in input {
        let c = *c as char;
        let digit = c.to_digit(10)?;
        num = num.checked_mul(10).and_then(|v| v.checked_add(digit))?;
    }
    Some(num)
}

pub fn parse_i32(input: &[u8]) -> Option<i32> {
    if input.is_empty() {
        return None;
    }

    let mut negative = false;
    let mut input = input;

    if input[0] == b'-' {
        negative = true;
        input = &input[1..];

        if input.is_empty() {
            return None;
        }
    }

    let mut num: i32 = 0;
    for c in input {
        let c = *c as char;
        let digit = c.to_digit(10)?;
        num = num
            .checked_mul(10)
            .and_then(|v| v.checked_add(digit as i32))?;
    }

    if negative {
        num = num.checked_neg()?;
    }

    Some(num)
}

#[cfg(test)]
#[path = "util_tests.rs"]
mod tests;
