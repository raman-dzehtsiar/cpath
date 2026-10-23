using System;
using System.Threading.Tasks;
class Example
{
    async Task<int> Work(IDisposable resource, object gate, Task<int> work)
    {
        using (resource)
        {
            lock (gate) { }
            try { return await work; }
            catch (Exception) { throw; }
            finally { }
        }
    }
}
