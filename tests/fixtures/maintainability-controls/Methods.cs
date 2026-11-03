// SPDX-License-Identifier: MIT
using System;
using System.Linq;
public class Examples
{
    private int field;
    public int Add(int a, int b) => a + b;
    public int Identity(int a) => a;
    public void Empty() { }
    public int Constant() => 42;
    public int Branch(int x) { if (x > 0) return x; return -x; }
    public int Loop(int n) { int sum = 0; for (int i = 0; i < n; i++) { sum += i; } return sum; }
    public void Set(int x) { field = x; }
    public int Get() => field;
    public void Print(string s) { Console.WriteLine(s); }
    public string Call(int n) => n.ToString();
    public int Local(int n) { int Inc(int x) => x + 1; return Inc(n); }
    public int Lambda(int[] values) => values.Where(x => x > 0).Sum();
    public int Catch(int x) { try { return 10 / x; } catch (Exception) { return 0; } }
    public int Switch(int x) { switch (x) { case 0: return 1; case 1: return 2; default: return 3; } }
    public string Choose(bool x, string a, string b) => x ? a : b;
    public string Coalesce(string a, string b) => a ?? b;
    public int Assign() { var x = 1; x = 2; return x; }
    public int Fold() => 1 + 2;
    public string Text(int n) => $"value {n}";
    public int Property { get => field; set => field = value; }
    public int ArrowProperty => field;
    public int Default(int x = 4) => x;
    [Obsolete("example")]
    public int Attribute(int x) => x;
    public Examples Fluent() { field = 2; return this; }
    public bool TypeTest(object value) => value is Examples;
    public Type TypeObject() => typeof(Examples);
    public void ConditionalVoid(Examples value) => value?.Set(1);
    private Func<int> callback = () => 1;
    public int DelegateCall() => callback();
    public int Each(int[] values) { int sum = 0; foreach (var value in values) sum += value; return sum; }
    public int SwitchExpression(int x) => x switch { 1 => 2, _ => 0 };
    public int Query(int[] values) { var query = from value in values where value > 0 select value; return query.Sum(); }
    [Obsolete("property")]
    public int AnnotatedProperty => field;
    public DateTime StaticPropertyReceiver() => DateTime.UtcNow.AddSeconds(8);
    [Obsolete]
    public int NoArgumentAttribute() => field;
    public int[] ArraySize() => new int[field + 1];
    public void EarlyReturn(bool b) { if (b) { return; } Set(1); }
    public Examples() : this(1) { }
    public Examples(int value) { field = value; }
    public ref int Reference() => ref field;
    public void RefAssignment(int value) { Reference() = value; if (field > 0) Reference() += 1; }
}
public class Primary(int value = 1, int other = 1)
{
    public int Sum() => value + other;
}
