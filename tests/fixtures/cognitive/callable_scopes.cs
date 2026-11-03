// SPDX-License-Identifier: MIT
using System;

class CallableScopes {
    private int value;
    public CallableScopes(int initial) {
        if (initial < 0) throw new ArgumentOutOfRangeException();
        value = initial;
    }

    public int Value {
        get {
            if (value < 0) return 0;
            return value;
        }
        set {
            if (value < 0) throw new ArgumentOutOfRangeException();
            this.value = value;
        }
    }

    public int Positive => value > 0 ? value : 0;
    public int this[int offset] => offset > 0 ? value + offset : value;

    public void Outer(bool active) {
        if (active) {
            int Local(int remaining) {
                while (remaining > 0) {
                    if (remaining == 2) return remaining;
                    remaining--;
                }
                return 0;
            }
            Func<int, int> choose = number => number > 0 ? number : 0;
            Action run = delegate() { if (active) Console.WriteLine(Local(3)); };
            run();
            Console.WriteLine(choose(2));
        }
    }

    public static CallableScopes operator +(CallableScopes left, CallableScopes right)
        => left.value > right.value ? left : right;

    public static explicit operator int(CallableScopes item)
        => item.value < 0 ? 0 : item.value;

    ~CallableScopes() {
        if (value > 0) Console.WriteLine(value);
    }
}
