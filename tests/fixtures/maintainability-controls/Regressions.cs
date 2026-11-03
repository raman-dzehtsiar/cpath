// SPDX-License-Identifier: MIT
using System;

[AttributeUsage(AttributeTargets.Parameter)]
public sealed class MarkerAttribute : Attribute { }
public class Base
{
    public Base() { }
    public Base(int value) { }
    public Base(object value) { }
}
public class Plain(int value = 1) { }
public class PrimaryBase(int value = 1) : Base(value) { }
public class PrimaryImplicit(int value = 1) : Base { }
public class PrimaryNull(object value = null) : Base(value) { }
public class PrimaryNullPlain(object value = null) { }
public class PrimaryBool(bool value = false) : Base { }
public record Pair(string First, string Second)
{
    public string Both() => $"{First.ToString()} {Second.ToString()}";
}
public static class RegressionCases
{
    public static int EmptyLambdas()
    {
        Action first = () => { };
        Action<int,int> second = (_, _) => { };
        first();
        second(1,2);
        return 0;
    }
    public static int LambdaReturn()
    {
        Func<int> f = () => { return 1; };
        return f();
    }
    public static int Parameters([Marker] int value) => value;
    public static int ParameterArray([Marker] params int[] values) => values.Length;
    public static int NameOf() => nameof(NameOf).Length;
    public static int VoidArrow() => throw new InvalidOperationException(nameof(VoidArrow));
    public static int DefaultValue() => default(int);
}
