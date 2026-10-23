using System.Threading.Tasks;

class AwaitRegions
{
    int value;
    int other;
    Task Pause() => Task.CompletedTask;
    void Observe(int item) { }

    async Task<int> SharedAcrossAwait()
    {
        this.value = 1;
        await Pause();
        return this.value;
    }

    async Task<int> RenamedMember()
    {
        this.other = 1;
        await Pause();
        return this.other;
    }

    async Task<int> AwaitInAssignment()
    {
        this.value = await Task.FromResult(1);
        return this.value;
    }

    async Task<int> DifferentMember()
    {
        this.value = 1;
        await Pause();
        return this.other;
    }

    async Task<int> LocalSnapshot()
    {
        int snapshot = this.value;
        await Pause();
        return snapshot;
    }

    async Task<int> ReadOnlyInAwaitOperand()
    {
        this.value = 1;
        return await Task.FromResult(this.value);
    }

    async Task<int> OverwriteAfterAwait()
    {
        this.value = 1;
        await Pause();
        this.value = 2;
        return this.value;
    }

    async Task ExclusiveBranches(bool flag)
    {
        if (flag)
        {
            this.value = 1;
            await Pause();
        }
        else
        {
            Observe(this.value);
        }
    }

    async Task<int> NestedCallableBoundary()
    {
        System.Action action = () => { this.value = 1; };
        await Pause();
        return this.value;
    }

    async Task PureWriteAfterAwait()
    {
        this.value = 1;
        await Pause();
        this.value = 2;
    }

    async Task<string> NameofAfterAwait()
    {
        this.value = 1;
        await Pause();
        return nameof(this.value);
    }

    async Task MethodNameAfterAwait()
    {
        this.value = 1;
        await Pause();
        this.Observe(1);
    }
}
