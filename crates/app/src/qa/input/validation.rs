use super::InputAction;

impl InputAction {
    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Move(request)
            | Self::Press(request)
            | Self::Release(request)
            | Self::Click(request)
            | Self::DoubleClick(request) => {
                if request.point().is_finite() {
                    Ok(())
                } else {
                    Err("coordinates must be finite numbers")
                }
            }
            Self::Drag(request) => {
                if !request.from.is_finite() || !request.to.is_finite() {
                    return Err("coordinates must be finite numbers");
                }
                if !(1..=120).contains(&request.steps) {
                    return Err("drag steps must be between 1 and 120");
                }
                Ok(())
            }
            Self::Key(_) | Self::CloseRequest => Ok(()),
            Self::Text(request) => {
                if request.text.len() > 4096 {
                    Err("text input must be at most 4096 UTF-8 bytes")
                } else {
                    Ok(())
                }
            }
            Self::Scroll(request) => {
                if request.x.is_finite()
                    && request.y.is_finite()
                    && request.delta_x.is_finite()
                    && request.delta_y.is_finite()
                {
                    Ok(())
                } else {
                    Err("scroll coordinates and deltas must be finite numbers")
                }
            }
            Self::Pinch(request) => {
                if !request.x.is_finite() || !request.y.is_finite() {
                    return Err("pinch coordinates must be finite numbers");
                }
                if !request.factor.is_finite() || !(0.01..=100.0).contains(&request.factor) {
                    return Err("pinch factor must be between 0.01 and 100");
                }
                Ok(())
            }
        }
    }
}
