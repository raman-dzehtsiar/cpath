// SPDX-License-Identifier: MIT
using System;

public static class OperationRegressions
{
    private static Func<int, int> Factory(int seed) => value => value + seed;
    public static int NestedInvocation(int value) => Factory(1)(value);

    public static ReadOnlySpan<byte> Utf8 => "one"u8 + "two"u8 + "one"u8;
    public static ReadOnlySpan<byte> RawUtf8 => """raw"""u8;

    public static int Uninitialized(int value)
    {
        int first;
        int second;
        Assign(out first, out second);
        return first + second + value;
    }

    private static void Assign(out int first, out int second)
    {
        first = 1;
        second = 2;
    }

    public static int NestedConditional(bool first, bool second, bool third)
        => (int)(first ? 1 : second ? 2 : third ? 3 : 4);
}
