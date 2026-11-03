// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;

public class IdentityFirst { public static readonly object Data = new(); }
public class IdentitySecond { public static readonly object Data = new(); }
public class IdentityThird { public static readonly object Data = new(); }

public static class IdentityCases
{
    public static IEnumerable<object[]> Cases
    {
        get
        {
            yield return new object[] { typeof(IdentityFirst), IdentityFirst.Data };
            yield return new object[] { typeof(IdentitySecond), IdentitySecond.Data };
            yield return new object[] { typeof(IdentityThird), IdentityThird.Data };
        }
    }

    public static IEnumerable<Type> Types
    {
        get
        {
            yield return typeof(sbyte);
            yield return typeof(byte);
            yield return typeof(short);
            yield return typeof(ushort);
            yield return typeof(int);
            yield return typeof(uint);
            yield return typeof(long);
            yield return typeof(ulong);
        }
    }
}
