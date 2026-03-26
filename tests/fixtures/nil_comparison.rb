# frozen_string_literal: true

def example
  x = get_value

  # These should trigger R061
  if x == nil
    puts "nil"
  end

  if x != nil
    puts "not nil"
  end

  # These should NOT trigger R061
  if x.nil?
    puts "nil"
  end

  if x == 0
    puts "zero"
  end
end
