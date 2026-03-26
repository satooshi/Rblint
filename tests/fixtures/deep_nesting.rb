# frozen_string_literal: true

def shallow
  if true
    if true
      puts "ok"
    end
  end
end

def too_deep
  if a
    if b
      if c
        if d
          if e
            puts "too deep"
          end
        end
      end
    end
  end
end

class Outer
  module Inner
    def method_in_nested_class
      if a
        if b
          if c
            if d
              puts "counted from method, not module"
            end
          end
        end
      end
    end
  end
end
