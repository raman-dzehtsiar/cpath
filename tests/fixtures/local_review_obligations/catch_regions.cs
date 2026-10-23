using System;

class CatchRegions
{
    void Work() { }

    void NamedThrow()
    {
        try { Work(); }
        catch (Exception caught) { throw caught; }
    }

    void RenamedBinding()
    {
        try { Work(); }
        catch (Exception renamed) { throw renamed; }
    }

    void BareRethrow()
    {
        try { Work(); }
        catch (Exception caught) { throw; }
    }

    void WrappedException()
    {
        try { Work(); }
        catch (Exception caught)
        {
            throw new InvalidOperationException("context", caught);
        }
    }

    void EmptyCatch()
    {
        try { Work(); }
        catch (Exception) { }
    }

    void CommentOnlyCatch()
    {
        try { Work(); }
        catch (Exception)
        {
            // Intentional best-effort cleanup; this comment changes no syntax.
        }
    }

    int ConstantFallback()
    {
        try { Work(); return 1; }
        catch (Exception) { return 0; }
    }

    void NestedCallableBoundary()
    {
        try { Work(); }
        catch (Exception caught)
        {
            Action action = () => { throw caught; };
        }
    }

    void ReboundException()
    {
        try { Work(); }
        catch (Exception caught)
        {
            caught = new InvalidOperationException();
            throw caught;
        }
    }
}
