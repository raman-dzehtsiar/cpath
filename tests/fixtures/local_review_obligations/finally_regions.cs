using System;

class FinallyRegions
{
    void Work() { }

    void ThrowInFinally()
    {
        try { Work(); }
        finally { throw new InvalidOperationException(); }
    }

    void ThrowInTry()
    {
        try { throw new InvalidOperationException(); }
        finally { Work(); }
    }

    void NestedCallableBoundary()
    {
        try { Work(); }
        finally
        {
            Action action = () => { throw new InvalidOperationException(); };
        }
    }

    void ConditionalFinallyThrow(bool flag)
    {
        try { Work(); }
        finally
        {
            if (flag) throw new InvalidOperationException();
        }
    }

    void LocallyCaughtThrow()
    {
        try { Work(); }
        finally
        {
            try { throw new InvalidOperationException(); }
            catch (InvalidOperationException) { Work(); }
        }
    }
}
