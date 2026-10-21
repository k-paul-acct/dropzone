use qrcode::{Color, QrCode};

const QUIET_ZONE: usize = 4;

pub fn render_to_string(code: &QrCode) -> String {
    render_modules(code.width(), |x, y| code[(x, y)] == Color::Dark)
}

fn render_modules(width: usize, is_dark: impl Fn(usize, usize) -> bool) -> String {
    let padded_width = width + QUIET_ZONE * 2;
    let terminal_height = padded_width.div_ceil(2);
    let mut output = String::with_capacity(padded_width * terminal_height * 5 / 2);

    for terminal_y in 0..terminal_height {
        if terminal_y > 0 {
            output.push('\n');
        }

        let upper_y = terminal_y * 2;
        let lower_y = upper_y + 1;

        for x in 0..padded_width {
            let upper = module_is_dark(x, upper_y, width, &is_dark);
            let lower = module_is_dark(x, lower_y, width, &is_dark);

            output.push(match (upper, lower) {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                (false, false) => ' ',
            });
        }
    }

    output
}

fn module_is_dark(padded_x: usize, padded_y: usize, width: usize, is_dark: &impl Fn(usize, usize) -> bool) -> bool {
    let Some(x) = padded_x.checked_sub(QUIET_ZONE) else {
        return false;
    };
    let Some(y) = padded_y.checked_sub(QUIET_ZONE) else {
        return false;
    };
    x < width && y < width && is_dark(x, y)
}
