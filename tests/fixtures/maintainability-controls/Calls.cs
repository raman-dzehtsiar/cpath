// SPDX-License-Identifier: MIT
using System.Collections.Generic;

public class LocalCallTarget
{
    public int Value() => 1;
    public static int StaticValue() => 2;
    public void Empty() { }
}

public class LocalCallCases
{
    public void Discarded(LocalCallTarget target)
    {
        target.Value();
        target.Empty();
    }

    public void Static() { LocalCallTarget.StaticValue(); }
    public static string Add(int value) => "local";
    public void Collection(List<int> values) { values.Add(1); }
}
