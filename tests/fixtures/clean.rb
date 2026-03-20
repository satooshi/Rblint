# frozen_string_literal: true

# This file should produce ZERO lint violations.

class Calculator
  MAX_RESULT = 1_000_000

  def initialize(precision)
    @precision = precision
  end

  def add(a, b)
    a + b
  end

  def subtract(a, b)
    a - b
  end

  def multiply(a, b)
    a * b
  end

  def valid_input?(value)
    value.is_a?(Numeric)
  end
end
