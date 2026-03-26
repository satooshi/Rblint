# frozen_string_literal: true

def example
  used = 1
  unused = 2
  _ignored = 3
  puts used
end

def with_block
  [1, 2].each do |item|
    temp = item * 2
    puts item
  end
end

def params(a, b, _c)
  puts a
end
