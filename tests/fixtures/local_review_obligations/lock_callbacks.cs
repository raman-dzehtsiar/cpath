using System;

class LockCallbacks
{
    readonly object gate = new object();
    void Helper() { }

    void UnderLock(Action callback)
    {
        lock (gate) { callback(); }
    }

    void RenamedParameter(Action operation)
    {
        lock (gate) { operation(); }
    }

    void OutsideLock(Action callback)
    {
        callback();
        lock (gate) { Helper(); }
    }

    void HelperUnderLock(Action callback)
    {
        lock (gate) { Helper(); }
    }

    void NestedCallableBoundary(Action callback)
    {
        lock (gate)
        {
            Action nested = () => { callback(); };
        }
    }

    string NameofUnderLock(Action callback)
    {
        lock (gate) { return nameof(callback); }
    }

    void ConditionalInvoke(Action callback)
    {
        lock (gate) { callback?.Invoke(); }
    }

    void MemberInvoke(Action callback)
    {
        lock (gate) { callback.Invoke(); }
    }

    void AliasUnderLock(Action callback)
    {
        Action alias = callback;
        lock (gate) { alias(); }
    }

    void ReboundParameter(Action callback)
    {
        callback = () => { };
        lock (gate) { callback(); }
    }

    void IsolatedGate(Action callback)
    {
        lock (new object()) { callback(); }
    }

    void LockExpression(Func<object> callback)
    {
        lock (callback()) { Helper(); }
    }
}
