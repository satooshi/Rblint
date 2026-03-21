# This is a sample Ruby file with intentional issues for rblint demo

class myClass  # R011: constant should start with uppercase... wait, class names do
  MAX_SIZE = 100

  def badMethodName(x, y,)  # R010: should be bad_method_name; R022: trailing comma
    result = x+y  # R021: missing space around +
    myVar = result * 2  # R012: camelCase variable
    return myVar  # R032: redundant return
  end

  def long_method
    a = 1
    b = 2
    c = 3
    d = 4
    e = 5
    f = 6
    g = 7
    h = 8
    i = 9
    j = 10
    k = 11
    l = 12
    m = 13
    n = 14
    o = 15
    p = 16
    q = 17
    r = 18
    s = 19
    t = 20
    u = 21
    v = 22
    w = 23
    x = 24
    y = 25
    z = 26
    aa = 27
    bb = 28
    cc = 29
    dd = 30
    ee = 31  # R040: method too long
    a + ee
  end

  def calculate(x)  # R021 will trigger here
    if x>0  # R021: missing space around >
      x * 2
    elsif x<0  # R021: missing space around <
      x * -1
    else
      0
    end
  end
end

# R003: missing frozen_string_literal comment at top
name = "world"
puts "Hello, #{name}!"   # R002: trailing whitespace on this line
