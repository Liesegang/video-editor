import math as math


class _RuvieVector(tuple):
    __slots__ = ()

    @property
    def x(self):
        return self[0]

    @property
    def y(self):
        return self[1]

    @property
    def z(self):
        return self[2]

    @property
    def w(self):
        return self[3]

    def _binary(self, other, operation):
        if isinstance(other, (tuple, list)):
            if len(self) != len(other):
                raise ValueError("vector dimensions must match")
            return _RuvieVector(operation(a, b) for a, b in zip(self, other))
        return _RuvieVector(operation(a, other) for a in self)

    def __add__(self, other):
        return self._binary(other, lambda a, b: a + b)

    def __radd__(self, other):
        return self.__add__(other)

    def __sub__(self, other):
        return self._binary(other, lambda a, b: a - b)

    def __rsub__(self, other):
        return _RuvieVector(other - value for value in self)

    def __mul__(self, other):
        return self._binary(other, lambda a, b: a * b)

    def __rmul__(self, other):
        return self.__mul__(other)

    def __truediv__(self, other):
        return self._binary(other, lambda a, b: a / b)

    def __neg__(self):
        return _RuvieVector(-value for value in self)


def _vector(size, values):
    if len(values) == 1 and isinstance(values[0], (tuple, list)):
        values = values[0]
    if len(values) != size:
        raise TypeError(f"vec{size} expects {size} components")
    return _RuvieVector(float(value) for value in values)


def vec2(*values):
    return _vector(2, values)


def vec3(*values):
    return _vector(3, values)


def vec4(*values):
    return _vector(4, values)


def rgba(r, g, b, a=1.0):
    return vec4(r, g, b, a)


def rgb(r, g, b):
    return rgba(r, g, b, 1.0)


def clamp(value, minimum, maximum):
    if isinstance(value, (tuple, list)):
        return _RuvieVector(clamp(component, minimum, maximum) for component in value)
    return min(max(value, minimum), maximum)


def lerp(start, end, amount):
    if isinstance(start, (tuple, list)):
        if not isinstance(end, (tuple, list)) or len(start) != len(end):
            raise TypeError("lerp vector dimensions must match")
        return _RuvieVector(lerp(a, b, amount) for a, b in zip(start, end))
    return start + (end - start) * amount


def smoothstep(edge0, edge1, value):
    if edge0 == edge1:
        raise ValueError("smoothstep edges must differ")
    position = clamp((value - edge0) / (edge1 - edge0), 0.0, 1.0)
    return position * position * (3.0 - 2.0 * position)


def dot(left, right):
    if len(left) != len(right):
        raise ValueError("dot vector dimensions must match")
    return sum(a * b for a, b in zip(left, right))


def length(value):
    return math.sqrt(dot(value, value))


def normalize(value):
    magnitude = length(value)
    if magnitude == 0:
        raise ZeroDivisionError("cannot normalize a zero-length vector")
    return _RuvieVector(component / magnitude for component in value)


def _unit_random(seed):
    mask = (1 << 64) - 1
    value = (int(seed) + 0x9E3779B97F4A7C15) & mask
    value = ((value ^ (value >> 30)) * 0xBF58476D1CE4E5B9) & mask
    value = ((value ^ (value >> 27)) * 0x94D049BB133111EB) & mask
    value ^= value >> 31
    return (value >> 11) / float(1 << 53)


def random(seed):
    return _unit_random(seed)


def noise(value, seed=0):
    # Stable scalar value noise suitable for authored animation expressions.
    integer = math.floor(float(value))
    fraction = float(value) - integer

    mask = (1 << 64) - 1

    def sample(position):
        return _unit_random(int(seed) ^ ((int(position) * 0x9E3779B97F4A7C15) & mask))

    smooth = fraction * fraction * (3.0 - 2.0 * fraction)
    return lerp(sample(integer), sample(integer + 1), smooth) * 2.0 - 1.0


pi = math.pi
sin = math.sin
cos = math.cos
tan = math.tan
atan2 = math.atan2
floor = math.floor
ceil = math.ceil
fmod = math.fmod
sqrt = math.sqrt
abs = abs
min = min
max = max
round = round
