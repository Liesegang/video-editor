
# 1D Perlin-like Noise Expression (Fractal Brownian Motion)
# Since we are using an expression evaluator, we can't define functions.
# We simulate noise by stacking sine waves at increasing frequencies and decreasing amplitudes.

# Structure:
# sum( sin(t * f + phase) * amplitude )
# where f = 2^i, amplitude = 0.5^i

(
    math.sin(t * 10.0 + 1.2) * 0.5 + 
    math.sin(t * 20.0 + 4.3) * 0.25 + 
    math.sin(t * 40.0 + 2.1) * 0.125 + 
    math.sin(t * 80.0 + 9.5) * 0.0625
) * 1.0  # Optional scale factor


# Random
random.Random(t).random() * 500