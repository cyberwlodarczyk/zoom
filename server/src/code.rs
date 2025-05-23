use rand::Rng;

pub fn is_valid(code: &str) -> bool {
    if code.len() != 11 {
        return false;
    }
    for (i, c) in code.bytes().enumerate() {
        if i == 3 || i == 7 {
            if c != b'-' {
                return false;
            }
        } else if c < b'a' || c > b'z' {
            return false;
        }
    }
    return true;
}

pub fn generate() -> String {
    let mut rng = rand::rng();
    let mut code = String::with_capacity(11);
    for i in 0..9 {
        let c = rng.random_range(b'a'..b'z');
        code.push(c as char);
        if i == 2 || i == 5 {
            code.push(b'-' as char);
        }
    }
    return code;
}
