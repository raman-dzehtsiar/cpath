// SPDX-License-Identifier: MIT
using System;
using System.Diagnostics.CodeAnalysis;

[method: Obsolete("old")]
public class PrimaryObsolete() { }

[method: SuppressMessage("Category", "Rule", Justification = "why")]
public class PrimarySuppression() { }

[method: Obsolete("old")]
[method: SuppressMessage("Category", "Rule", Justification = "why")]
public class PrimaryBoth() { }

[Obsolete("old")]
[SuppressMessage("Category", "Rule", Justification = "why")]
public class TypeOnly() { }

public class OrdinaryAttributes
{
    [Obsolete("old")]
    [SuppressMessage("Category", "Rule", Justification = "why")]
    public OrdinaryAttributes() { }
}

public class AttributeBase { public AttributeBase(int value) { } }

[method: Obsolete("old")]
[method: SuppressMessage("Category", "Rule", Justification = "why")]
public class PrimaryBaseAttributes(int value = 1) : AttributeBase(value) { }

[method: Obsolete("old")]
public record PrimaryRecordAttributes(int Value);
