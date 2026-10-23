using System;

class UsingEscape
{
    sealed class Resource : IDisposable
    {
        public int Value => 1;
        public void Dispose() { }
    }

    Resource DirectReturn()
    {
        using (var resource = new Resource())
        {
            return resource;
        }
    }

    Resource RenamedBinding()
    {
        using (var renamed = new Resource())
        {
            return renamed;
        }
    }

    int ReturnValue()
    {
        using (var resource = new Resource())
        {
            return resource.Value;
        }
    }

    Resource ReturnOtherObject()
    {
        using (var resource = new Resource())
        {
            return new Resource();
        }
    }

    Resource AliasEscape()
    {
        using (var resource = new Resource())
        {
            var alias = resource;
            return alias;
        }
    }

    Resource DeclarationForm()
    {
        using var resource = new Resource();
        return resource;
    }

    Func<Resource> DeferredCapture()
    {
        using (var resource = new Resource())
        {
            return () => resource;
        }
    }
}
