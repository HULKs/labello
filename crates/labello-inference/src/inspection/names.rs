use std::collections::BTreeMap;

/// Ultralytics writes a Python dictionary literal, not JSON. Parse only integer
/// keys and quoted strings; never evaluate metadata as code.
pub(super) fn parse(value: &str) -> Result<Vec<String>, ()> {
    let mut input = value.chars().peekable();
    let mut next = |expected| {
        while input.peek().is_some_and(|c| char::is_whitespace(*c)) {
            input.next();
        }
        if input.next() == Some(expected) {
            Ok(())
        } else {
            Err(())
        }
    };
    next('{')?;
    let mut names = BTreeMap::new();
    loop {
        whitespace(&mut input);
        if input.peek() == Some(&'}') {
            input.next();
            break;
        }
        let key = if matches!(input.peek(), Some('\'' | '"')) {
            quoted(&mut input)?
        } else {
            let mut key = String::new();
            while input.peek().is_some_and(char::is_ascii_digit) {
                key.push(input.next().ok_or(())?);
            }
            key
        };
        let id: usize = key.parse().map_err(|_| ())?;
        whitespace(&mut input);
        if input.next() != Some(':') {
            return Err(());
        }
        whitespace(&mut input);
        let name = quoted(&mut input)?;
        if id >= 1000
            || name.len() > 256
            || name.trim().is_empty()
            || names.insert(id, name).is_some()
        {
            return Err(());
        }
        whitespace(&mut input);
        match input.peek() {
            Some(',') => {
                input.next();
            }
            Some('}') => {}
            _ => return Err(()),
        }
    }
    whitespace(&mut input);
    if input.next().is_some() || names.is_empty() || !names.keys().copied().eq(0..names.len()) {
        return Err(());
    }
    Ok(names.into_values().collect())
}

fn whitespace(input: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while input.peek().is_some_and(|c| c.is_whitespace()) {
        input.next();
    }
}

fn quoted(input: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Result<String, ()> {
    let quote = input.next().filter(|c| matches!(c, '\'' | '"')).ok_or(())?;
    let mut value = String::new();
    while let Some(c) = input.next() {
        if c == quote {
            return Ok(value);
        }
        if c != '\\' {
            value.push(c);
            continue;
        }
        let c = input.next().ok_or(())?;
        value.push(match c {
            '\'' | '"' | '\\' | '/' => c,
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'b' => '\x08',
            'f' => '\x0c',
            'u' | 'U' | 'x' => {
                let digits = match c {
                    'u' => 4,
                    'U' => 8,
                    _ => 2,
                };
                let mut code = 0u32;
                for _ in 0..digits {
                    code = code.checked_mul(16).ok_or(())?
                        + input.next().and_then(|c| c.to_digit(16)).ok_or(())?;
                }
                char::from_u32(code).ok_or(())?
            }
            _ => return Err(()),
        });
    }
    Err(())
}
